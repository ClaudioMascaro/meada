//! The operations on the generation pointer and the fence (ADR 0014, feature `refresh-lock`):
//! `meada snapshot pointer set` and `meada snapshot fence recover`, which the console calls
//! too. Each checks its arguments, takes the refresh lock as `meada snapshot lock` does
//! (`lock::acquire`: a fresh token, the holder naming the operation and its author, its own
//! fence), makes its one write through a script that checks the lock still holds its token, and
//! releases the lock and the holder whatever the write did.

use std::path::Path;

use redis::cmd;

use super::generations::{self, pointer};
use super::lock::{self, Held};
use super::{fence, is_id};
use crate::msg::Msg;
use crate::valkey::{self, Valkey};
use crate::workspace::Manifest;

/// Runs `op` under the refresh lock of `manifest`'s workspace, its holder saying `holds`, with
/// `[generations]`'s `keep`; the lock and the holder are released whatever `op` returned.
fn locked<T>(
    manifest: &Path,
    holds: &str,
    op: impl AsyncFnOnce(&Valkey, &valkey::Config, u64, &Held) -> Result<T, String>,
) -> crate::Result<T> {
    let listed = Manifest::read(manifest)?;
    let shown = manifest.display().to_string();
    // `[refresh_lock]` needs `[generations]`, which needs `[valkey]` (`Manifest::read`).
    let (Some(section), Some(config), Some(generations)) =
        (listed.refresh_lock, listed.valkey, listed.generations)
    else {
        return Err(Msg::LockNoSection(&shown).text().into());
    };
    let run = async {
        let valkey = Valkey::connect(&config).await?;
        let held = lock::acquire(&valkey, &config, section.ttl, holds).await?;
        let done = op(&valkey, &config, generations.keep, &held).await;
        lock::release(&valkey, &config, &held.token).await;
        done
    };
    Ok(crate::modules::block_on(run)??)
}

/// `meada snapshot pointer set <generation>`: points the workspace at `id`, one of the
/// generations nodes hold (the first `keep` of `{<key_prefix>}:retained`), with the operation's
/// fence, through the pointer script; `<id> <fence>`.
pub fn pointer_set(manifest: &Path, id: &str, author: &str) -> crate::Result<String> {
    if !is_id(id) {
        return Err(Msg::BadGenerationArg(&id.escape_debug().to_string())
            .text()
            .into());
    }
    locked(
        manifest,
        &format!("pointer set {author}"),
        async |valkey, config, keep, held| {
            let retained = generations::keys(config).1;
            let range = cmd("LRANGE").arg(&retained).arg(0).arg(keep - 1).clone();
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
        manifest,
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

/// The highest fence recorded in the pointer, the retained list and the heartbeat of every live
/// name of `{<key_prefix>}:nodes`, 0 for none. Everything is hostile: what fails a check is
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

/// Whether `name` is a node name: lowercase letters, digits, `.` and `-`, 1 to 63 characters.
// ponytail: #298 checks `node_name` against the same pattern; one function once both land.
fn is_node_name(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && (name.bytes()).all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-'))
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

    #[test]
    fn a_node_name_is_lowercase_letters_digits_dots_and_dashes() {
        for good in ["a", "meada-node-0", "node.example.com", &"a".repeat(63)] {
            assert!(is_node_name(good), "{good}");
        }
        for bad in ["", "Upper", "a_b", "a b", "../x", "a:b", &"a".repeat(64)] {
            assert!(!is_node_name(bad), "{bad}");
        }
    }
}
