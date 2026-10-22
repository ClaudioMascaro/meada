//! The operations on the generation pointer and the fence (ADR 0014, feature `refresh-lock`):
//! `meada snapshot pointer set`, `retain` and `drop`, and `meada snapshot fence recover`, which
//! the console calls too. Each checks its arguments, takes the refresh lock as `meada snapshot lock` does
//! (`lock::acquire`: a fresh token, the holder naming the operation and its author, its own
//! fence), makes its one write through a script that checks the lock still holds its token, and
//! releases the lock and the holder whatever the write did.

use std::path::Path;

use redis::cmd;

use super::generations::{self, KEPT_MAX, pointer};
use super::lock::{self, Held};
use super::store::Store;
use super::{fence, is_id, is_node_name, pull};
use crate::msg::Msg;
use crate::valkey::{self, Valkey};
use crate::workspace::Manifest;

/// The sections of a workspace the operations need.
pub(super) struct Sections {
    pub(super) lock: lock::Config,
    pub(super) valkey: valkey::Config,
    pub(super) keep: u64,
    pub(super) snapshot: super::Config,
}

/// `manifest`'s sections, or the error naming it when it has no `[refresh_lock]`.
pub(super) fn sections(manifest: &Path) -> crate::Result<Sections> {
    let listed = Manifest::read(manifest)?;
    // `[refresh_lock]` needs `[generations]`, which needs `[valkey]` and `[snapshot]`
    // (`Manifest::read`).
    match (
        listed.refresh_lock,
        listed.valkey,
        listed.generations,
        listed.snapshot,
    ) {
        (Some(lock), Some(valkey), Some(generations), Some(snapshot)) => Ok(Sections {
            lock,
            valkey,
            keep: generations.keep,
            snapshot,
        }),
        _ => Err(Msg::LockNoSection(&manifest.display().to_string())
            .text()
            .into()),
    }
}

/// Runs `op` under the refresh lock of the workspace of `sections`, its holder saying `holds`,
/// with `[generations]`'s `keep`; the lock and the holder are released whatever `op` returned.
pub(super) fn locked<T>(
    sections: &Sections,
    holds: &str,
    op: impl AsyncFnOnce(&Valkey, &valkey::Config, u64, &Held) -> Result<T, String>,
) -> crate::Result<T> {
    let config = &sections.valkey;
    let run = async {
        let valkey = Valkey::connect(config).await?;
        let held = lock::acquire(&valkey, config, sections.lock.ttl, holds).await?;
        let done = op(&valkey, config, sections.keep, &held).await;
        lock::release(&valkey, config, &held.token).await;
        done
    };
    Ok(crate::modules::block_on(run)??)
}

/// `id` when it is a generation id, else the refusal naming the argument.
fn generation_arg(id: &str) -> crate::Result<()> {
    match is_id(id) {
        true => Ok(()),
        false => Err(Msg::BadGenerationArg(&id.escape_debug().to_string())
            .text()
            .into()),
    }
}

/// `meada snapshot pointer set <generation>`: points the workspace at `id`, one of the
/// generations nodes hold (`{<key_prefix>}:retained`), with the operation's fence, through the
/// pointer script; `<id> <fence>`.
pub fn pointer_set(manifest: &Path, id: &str, author: &str) -> crate::Result<String> {
    generation_arg(id)?;
    locked(
        &sections(manifest)?,
        &format!("pointer set {author}"),
        async |valkey, config, keep, held| {
            let retained = generations::keys(config).1;
            let range = cmd("LRANGE").arg(&retained).arg(0).arg(-1).clone();
            let entries: Vec<Vec<u8>> =
                (valkey.query(&range).await).map_err(|e| format!("{retained}: {e}"))?;
            let mine = format!("{id}:");
            let listed = (entries.iter()).any(|e| {
                let rest = e.strip_prefix(mine.as_bytes());
                rest.and_then(|f| std::str::from_utf8(f).ok())
                    .and_then(fence)
                    .is_some()
            });
            if !listed {
                return Err(Msg::PointerNotRetained { id, key: &retained }.text());
            }
            generations::point(valkey, config, keep, id, Some(&held.token), held.fence).await?;
            Ok(format!("{id} {}", held.fence))
        },
    )
}

/// Retains and keeps a generation while the lock (`KEYS[1]`) holds the token `ARGV[1]`: 0
/// without the lock; 1 when the kept set (`KEYS[3]`) already holds the id `ARGV[2]`, nothing
/// changed; -1 when it already holds `ARGV[4]` ids; else, with no entry of the id in the
/// retained list (`KEYS[2]`), `<id>:<fence>` (`ARGV[3]`) appended at its oldest end, the id
/// added to the kept set, and 1.
const RETAIN: &str = r"
if redis.call('GET', KEYS[1]) ~= ARGV[1] then
  return 0
end
if redis.call('SISMEMBER', KEYS[3], ARGV[2]) == 1 then
  return 1
end
if redis.call('SCARD', KEYS[3]) >= tonumber(ARGV[4]) then
  return -1
end
local mine = ARGV[2] .. ':'
local listed = false
for _, entry in ipairs(redis.call('LRANGE', KEYS[2], 0, -1)) do
  if string.sub(entry, 1, #mine) == mine then
    listed = true
  end
end
if not listed then
  redis.call('RPUSH', KEYS[2], mine .. ARGV[3])
end
redis.call('SADD', KEYS[3], ARGV[2])
return 1
";

/// `meada snapshot pointer retain <generation>`: once the store holds `id`'s document, checked as
/// a pull checks it, keeps the generation on every node outside the `keep` trim (`RETAIN`, with
/// the operation's fence); `id`.
pub fn pointer_retain(manifest: &Path, id: &str, author: &str) -> crate::Result<String> {
    generation_arg(id)?;
    let sections = sections(manifest)?;
    let check = async {
        let store = Store::connect(&sections.snapshot)?;
        pull::stored_document(&store, &sections.snapshot, id).await
    };
    crate::modules::block_on(check)??;
    locked(
        &sections,
        &format!("pointer retain {author}"),
        async |valkey, config, _, held| {
            let (lock, retained, kept) = (
                config.key("lock", true),
                generations::keys(config).1,
                generations::kept(config),
            );
            let mut script = cmd("EVAL");
            script
                .arg(RETAIN)
                .arg(3)
                .arg(&lock)
                .arg(&retained)
                .arg(&kept);
            script
                .arg(&held.token)
                .arg(id)
                .arg(held.fence)
                .arg(KEPT_MAX);
            let wrote = valkey.query::<i64>(&script).await;
            match wrote.map_err(|e| format!("{retained}, {kept}: {e}"))? {
                1 => Ok(id.to_string()),
                -1 => Err(Msg::KeptFull {
                    key: &kept,
                    max: KEPT_MAX,
                }
                .text()),
                _ => Err(Msg::LockNotHeld(&lock).text()),
            }
        },
    )
}

/// Drops a generation while the lock (`KEYS[1]`) holds the token `ARGV[1]`: 0 without the lock,
/// -1 when the pointer (`KEYS[2]`) names the id `ARGV[2]`, -2 when the retained list (`KEYS[3]`)
/// has no entry of it; else every entry of it removed from the list and the id from the kept set
/// (`KEYS[4]`), and 1.
const DROP: &str = r"
if redis.call('GET', KEYS[1]) ~= ARGV[1] then
  return 0
end
if redis.call('HGET', KEYS[2], 'id') == ARGV[2] then
  return -1
end
local mine = ARGV[2] .. ':'
local listed = false
for _, entry in ipairs(redis.call('LRANGE', KEYS[3], 0, -1)) do
  if string.sub(entry, 1, #mine) == mine then
    redis.call('LREM', KEYS[3], 0, entry)
    listed = true
  end
end
if not listed then
  return -2
end
redis.call('SREM', KEYS[4], ARGV[2])
return 1
";

/// `meada snapshot pointer drop <generation>`: removes `id`, retained and not current, from the
/// retained list and the kept set (`DROP`), so the next pull deletes it; `id`.
pub fn pointer_drop(manifest: &Path, id: &str, author: &str) -> crate::Result<String> {
    generation_arg(id)?;
    locked(
        &sections(manifest)?,
        &format!("pointer drop {author}"),
        async |valkey, config, _, held| {
            let lock = config.key("lock", true);
            let ((current, retained), kept) =
                (generations::keys(config), generations::kept(config));
            let mut script = cmd("EVAL");
            script.arg(DROP).arg(4).arg(&lock).arg(&current);
            script.arg(&retained).arg(&kept).arg(&held.token).arg(id);
            let wrote = valkey.query::<i64>(&script).await;
            match wrote.map_err(|e| format!("{current}, {retained}, {kept}: {e}"))? {
                1 => Ok(id.to_string()),
                -1 => Err(Msg::DropCurrent { id, key: &current }.text()),
                -2 => Err(Msg::DropNotRetained { id, key: &retained }.text()),
                _ => Err(Msg::LockNotHeld(&lock).text()),
            }
        },
    )
}

/// Raises the fence to `ARGV[2]` while the lock (`KEYS[1]`) holds the token `ARGV[1]`, never
/// lowering it: the fence (`KEYS[2]`) it leaves, compared as decimal text; nil without the lock.
const RAISE: &str = r"
if redis.call('GET', KEYS[1]) ~= ARGV[1] then
  return false
end
local held = redis.call('GET', KEYS[2])
if held and string.match(held, '^%d+$') then
  held = (string.gsub(held, '^0+(%d)', '%1'))
  local fence = ARGV[2]
  if #fence < #held or (#fence == #held and fence <= held) then
    return held
  end
end
redis.call('SET', KEYS[2], ARGV[2])
return ARGV[2]
";

/// `meada snapshot fence recover [--at-least <fence>]`: raises `{<key_prefix>}:fence` to the
/// highest fence recorded (the pointer, every retained entry, every live heartbeat) or to
/// `at_least` when higher, never lowering it. The fence it leaves, and a line for each record
/// it could not read, which does not count.
pub fn fence_recover(
    manifest: &Path,
    at_least: Option<&str>,
    author: &str,
) -> crate::Result<(u64, Vec<String>)> {
    let floor = match at_least {
        None => 0,
        Some(text) => {
            fence(text).ok_or_else(|| Msg::BadFenceArg(&text.escape_debug().to_string()).text())?
        }
    };
    locked(
        &sections(manifest)?,
        &format!("fence recover {author}"),
        async |valkey, config, _, held| {
            let mut skipped = Vec::new();
            let highest = recorded(valkey, config, &mut skipped).await?.max(floor);
            let (lock, fence_key, _) = lock::keys(config);
            let mut raise = cmd("EVAL");
            raise.arg(RAISE).arg(2).arg(&lock).arg(&fence_key);
            raise.arg(&held.token).arg(highest);
            let left: Option<String> =
                (valkey.query(&raise).await).map_err(|e| format!("{fence_key}: {e}"))?;
            let left = left.ok_or_else(|| Msg::LockNotHeld(&lock).text())?;
            let left = fence(&left).ok_or_else(|| Msg::LockBadFence(&fence_key).text())?;
            Ok((left, skipped))
        },
    )
}

/// The highest fence recorded in the pointer, the workspace pointer, the retained list and the
/// heartbeat of every live name of `{<key_prefix>}:nodes`, 0 for none. Everything is hostile: what fails a check is
/// named in `skipped` and not counted.
async fn recorded(
    valkey: &Valkey,
    config: &valkey::Config,
    skipped: &mut Vec<String>,
) -> Result<u64, String> {
    let mut skip = |problem: String| skipped.push(Msg::FenceNotCounted(&problem).text());
    let (current, retained) = generations::keys(config);
    let mut fences = Vec::new();

    let fields = cmd("HMGET").arg(&current).arg("id").arg("fence").clone();
    let fields: Vec<Option<Vec<u8>>> =
        (valkey.query(&fields).await).map_err(|e| format!("{current}: {e}"))?;
    if fields.iter().any(Option::is_some) {
        match pointer(&current, &fields) {
            Ok((_, fence)) => fences.push(fence),
            Err(e) => skip(e),
        }
    }
    // The workspace pointer (`meada workspace apply`) takes its fence from the same counter.
    match super::versions::current(valkey, config).await {
        Ok(found) => fences.extend(found.map(|(_, fence)| fence)),
        Err(e) => skip(e),
    }

    let range = cmd("LRANGE").arg(&retained).arg(0).arg(-1).clone();
    let entries: Vec<Vec<u8>> =
        (valkey.query(&range).await).map_err(|e| format!("{retained}: {e}"))?;
    for entry in &entries {
        let entry = std::str::from_utf8(entry)
            .ok()
            .and_then(|e| e.split_once(':'));
        match entry
            .filter(|(id, _)| is_id(id))
            .and_then(|(_, f)| fence(f))
        {
            Some(fence) => fences.push(fence),
            None => skip(Msg::GenerationsBadEntry(&retained).text()),
        }
    }

    let nodes = config.key("nodes", true);
    let live = cmd("ZRANGEBYSCORE")
        .arg(&nodes)
        .arg(format!("({}", lock::now()))
        .arg("+inf")
        .clone();
    let names: Vec<Vec<u8>> = (valkey.query(&live).await).map_err(|e| format!("{nodes}: {e}"))?;
    for name in &names {
        let Some(name) = std::str::from_utf8(name).ok().filter(|n| is_node_name(n)) else {
            let name = String::from_utf8_lossy(name).escape_debug().to_string();
            skip(
                Msg::NodeNameBad {
                    key: &nodes,
                    name: &name,
                }
                .text(),
            );
            continue;
        };
        let key = config.key(&format!("node:{name}"), true);
        let get = cmd("GET").arg(&key).clone();
        let beat: Option<Vec<u8>> =
            (valkey.query(&get).await).map_err(|e| format!("{key}: {e}"))?;
        // A name whose key expired has no heartbeat: nothing to count.
        let Some(beat) = beat else { continue };
        match heartbeat_fence(&beat) {
            Some(fence) => fences.extend(fence),
            None => skip(Msg::HeartbeatBad(&key).text()),
        }
    }
    Ok(fences.into_iter().max().unwrap_or(0))
}

/// The fence of a heartbeat, a JSON object whose `fence` is a `u64` or null (a node with no
/// pointer file); `None` for anything else.
fn heartbeat_fence(beat: &[u8]) -> Option<Option<u64>> {
    let beat: serde_json::Value = serde_json::from_slice(beat).ok()?;
    match beat.as_object()?.get("fence")? {
        serde_json::Value::Null => Some(None),
        fence => fence.as_u64().map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heartbeat_s_fence_is_a_u64_or_null() {
        assert_eq!(heartbeat_fence(br#"{"name":"a","fence":7}"#), Some(Some(7)));
        assert_eq!(heartbeat_fence(br#"{"fence":null}"#), Some(None));
        for bad in [
            &br#"{"fence":"7"}"#[..],
            br#"{"fence":-1}"#,
            br#"{"fence":1e3}"#,
            br#"{"fence":18446744073709551616}"#,
            br#"{"name":"a"}"#,
            br#"[7]"#,
            b"7",
            b"{",
            &[0xff, 0xfe],
        ] {
            assert_eq!(
                heartbeat_fence(bad),
                None,
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}
