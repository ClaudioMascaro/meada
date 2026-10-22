//! The generation pointer (ADR 0012, feature `generation-pointer`, section `[generations]`):
//! the workspace's current generation and the retained ones in Valkey. `meada snapshot publish`
//! writes them (`point`, at its end, instead of the store's `current`) and every node's `meada
//! snapshot pull` reads them (`read`) and holds every retained generation, so the fleet holds
//! one set and a workflow run can pin one (`/<identity>@<generation>`, `mcp::route`).
//!
//! Keys, under the `[valkey]` key prefix and its hash tag (one slot in cluster mode):
//! `{<key_prefix>}:current`, a hash `{id, fence}`, and `{<key_prefix>}:retained`, a list of
//! `<id>:<fence>` newest first, each id once, trimmed to `keep`. Everything read back is hostile:
//! each id and fence is checked before it becomes part of a path, a key or a comparison.

use redis::cmd;
use serde::Deserialize;

use super::{fence, is_id};
use crate::msg::Msg;
use crate::valkey::{self, Valkey};

/// The fewest generations `keep` may retain: the current one and the one it replaced.
const KEEP_MIN: i64 = 2;
const KEEP_DEFAULT: i64 = 3;

/// The `[generations]` section as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    keep: Option<i64>,
}

/// The validated section.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Generations retained fleet-wide, at least `KEEP_MIN`.
    pub keep: u64,
}

impl File {
    pub fn validate(self) -> Result<Config, crate::modules::Problem> {
        let keep = crate::modules::at_least("generations.keep", self.keep, KEEP_DEFAULT, KEEP_MIN)?;
        Ok(Config { keep })
    }
}

/// The pointer's two keys.
pub fn keys(valkey: &valkey::Config) -> (String, String) {
    (valkey.key("current", true), valkey.key("retained", true))
}

/// The pointer script. KEYS: current, retained, lock. ARGV: id, fence, keep, lock token (empty
/// for none). With a token it returns 0 unless the lock holds it, and -1 unless the fence is
/// above the one `current` holds (compared as decimal text, so no fence loses precision as a
/// Lua number; -2 when that one is no decimal). Then it removes every entry of the id from the
/// list before pushing it, so the list holds each id once, trims the list to `keep` and sets
/// the hash, all at once. Without a token (no `[refresh_lock]`) the fence is `0` and nothing
/// is checked.
const POINT: &str = r"
if ARGV[4] ~= '' then
  if redis.call('GET', KEYS[3]) ~= ARGV[4] then
    return 0
  end
  local held = redis.call('HGET', KEYS[1], 'fence')
  if held then
    if not string.match(held, '^%d+$') then
      return -2
    end
    held = (string.gsub(held, '^0+(%d)', '%1'))
    local fence = ARGV[2]
    if #fence < #held or (#fence == #held and fence <= held) then
      return -1
    end
  end
end
local mine = ARGV[1] .. ':'
for _, entry in ipairs(redis.call('LRANGE', KEYS[2], 0, -1)) do
  if string.sub(entry, 1, #mine) == mine then
    redis.call('LREM', KEYS[2], 0, entry)
  end
end
redis.call('LPUSH', KEYS[2], mine .. ARGV[2])
redis.call('LTRIM', KEYS[2], 0, tonumber(ARGV[3]) - 1)
redis.call('HSET', KEYS[1], 'id', ARGV[1], 'fence', ARGV[2])
return 1
";

/// Points the workspace at the generation `id` with `fence`, retaining `keep` generations: the
/// last write of `meada snapshot publish`. `token` is the refresh lock's, `None` without one:
/// with it the write happens only while `{<key_prefix>}:lock` holds the token and `fence` is
/// above the current one's, and a refusal is an error naming the key that refused it.
pub async fn point(
    valkey: &Valkey,
    config: &valkey::Config,
    keep: u64,
    id: &str,
    token: Option<&str>,
    fence: u64,
) -> Result<(), String> {
    let (current, retained) = keys(config);
    let lock = config.key("lock", true);
    let mut script = cmd("EVAL");
    script
        .arg(POINT)
        .arg(3)
        .arg(&current)
        .arg(&retained)
        .arg(&lock);
    script.arg(id).arg(fence).arg(keep).arg(token.unwrap_or(""));
    let wrote = valkey.query::<i64>(&script).await;
    match wrote.map_err(|e| format!("{current}, {retained}: {e}"))? {
        1 => Ok(()),
        0 => Err(Msg::LockNotHeld(&lock).text()),
        -1 => Err(Msg::FenceRefused {
            fence,
            current: &current,
            fence_key: &config.key("fence", true),
        }
        .text()),
        _ => Err(Msg::GenerationsBadPointer(&current).text()),
    }
}

/// What Valkey says a node should hold.
#[derive(Debug, PartialEq)]
pub struct Pointer {
    /// The current generation and its fence.
    pub id: String,
    pub fence: u64,
    /// The retained generations, newest first, each once, at most `keep`.
    pub retained: Vec<String>,
}

/// The pointer and the first `keep` retained generations, every id and fence checked. A missing
/// pointer, an empty list or a malformed entry is an error naming its key.
pub async fn read(valkey: &Valkey, config: &valkey::Config, keep: u64) -> Result<Pointer, String> {
    let (id, fence) = current(valkey, config).await?;
    let retained = keys(config).1;
    let range = cmd("LRANGE").arg(&retained).arg(0).arg(keep - 1).clone();
    let entries: Vec<Vec<u8>> =
        (valkey.query(&range).await).map_err(|e| format!("{retained}: {e}"))?;
    Ok(Pointer {
        id,
        fence,
        retained: listed(&retained, &entries)?,
    })
}

/// The current generation and its fence, checked; a missing or malformed pointer is an error
/// naming its key.
pub async fn current(valkey: &Valkey, config: &valkey::Config) -> Result<(String, u64), String> {
    let current = keys(config).0;
    let fields = cmd("HMGET").arg(&current).arg("id").arg("fence").clone();
    let fields: Vec<Option<Vec<u8>>> =
        (valkey.query(&fields).await).map_err(|e| format!("{current}: {e}"))?;
    pointer(&current, &fields)
}

/// The hash `{<key_prefix>}:current` (`key`), as `HMGET key id fence` returns it.
pub(super) fn pointer(key: &str, fields: &[Option<Vec<u8>>]) -> Result<(String, u64), String> {
    let text = |bytes: &[u8]| std::str::from_utf8(bytes).ok().map(str::to_string);
    match fields {
        [None, None] => Err(Msg::SnapshotNoCurrent(key).text()),
        [Some(id), Some(f)] => match (
            text(id).filter(|id| is_id(id)),
            text(f).and_then(|f| fence(&f)),
        ) {
            (Some(id), Some(fence)) => Ok((id, fence)),
            _ => Err(Msg::GenerationsBadPointer(key).text()),
        },
        _ => Err(Msg::GenerationsBadPointer(key).text()),
    }
}

/// The ids of the list `{<key_prefix>}:retained` (`key`), each entry `<id>:<fence>`, in order and
/// each once.
fn listed(key: &str, entries: &[Vec<u8>]) -> Result<Vec<String>, String> {
    if entries.is_empty() {
        return Err(Msg::GenerationsEmpty(key).text());
    }
    let mut ids: Vec<String> = Vec::new();
    for entry in entries {
        let entry = std::str::from_utf8(entry).ok();
        let parsed = entry.and_then(|e| e.split_once(':'));
        let Some((id, _)) = parsed.filter(|(id, f)| is_id(id) && fence(f).is_some()) else {
            return Err(Msg::GenerationsBadEntry(key).text());
        };
        if !ids.iter().any(|held| held == id) {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "{meada}:current";
    const LIST: &str = "{meada}:retained";
    const A: &str = "000000000000000a";
    const B: &str = "000000000000000b";

    fn fields(id: Option<&str>, fence: Option<&str>) -> Vec<Option<Vec<u8>>> {
        vec![id.map(|s| s.into()), fence.map(|s| s.into())]
    }

    fn bad_pointer(id: Option<&str>, fence: Option<&str>) {
        let e = pointer(KEY, &fields(id, fence)).unwrap_err();
        assert!(e.starts_with(&format!("{KEY}: ")), "{e}");
        assert!(e.contains("decimal fence"), "{e}");
    }

    fn bad_entry(entry: &[u8]) {
        let entries = [format!("{A}:1").into_bytes(), entry.to_vec()];
        let e = listed(LIST, &entries).unwrap_err();
        assert!(e.starts_with(&format!("{LIST}: ")), "{e}");
    }

    #[test]
    fn the_section_defaults_to_three_and_refuses_fewer_than_two() {
        let section = |toml: &str| toml::from_str::<File>(toml).unwrap().validate();
        assert_eq!(section("").unwrap().keep, 3);
        assert_eq!(section("keep = 2").unwrap().keep, 2);
        for toml in ["keep = 1", "keep = 0", "keep = -4"] {
            let (key, message) = section(toml).unwrap_err();
            assert_eq!(
                (key, message.as_str()),
                ("generations.keep", "must be at least 2")
            );
        }
    }

    #[test]
    fn the_keys_share_the_prefix_s_hash_tag() {
        let config: valkey::File =
            toml::from_str("url = \"redis://127.0.0.1\"\nkey_prefix = \"meada:prod\"").unwrap();
        let config = config.validate(std::path::Path::new("/")).unwrap();
        assert_eq!(
            keys(&config),
            (
                "{meada:prod}:current".into(),
                "{meada:prod}:retained".into()
            )
        );
    }

    #[test]
    fn a_pointer_is_an_id_and_a_decimal_fence() {
        assert_eq!(pointer(KEY, &fields(Some(A), Some("0"))), Ok((A.into(), 0)));
        assert_eq!(
            pointer(KEY, &fields(Some(A), Some("42"))),
            Ok((A.into(), 42))
        );
    }

    #[test]
    fn a_missing_pointer_names_its_key() {
        let e = pointer(KEY, &fields(None, None)).unwrap_err();
        assert!(e.starts_with(&format!("{KEY}: not found")), "{e}");
    }

    #[test]
    fn a_pointer_id_that_is_not_an_id_is_refused() {
        for id in [
            "000000000000000A",
            "000000000000000",
            "0000000000000000a",
            "../../../../etc/",
            "",
        ] {
            bad_pointer(Some(id), Some("0"));
        }
    }

    #[test]
    fn a_pointer_id_with_a_path_separator_is_refused() {
        bad_pointer(Some("00000000/0000000"), Some("0"));
        bad_pointer(Some("0000000\\00000000"), Some("0"));
    }

    #[test]
    fn a_pointer_fence_that_is_not_decimal_is_refused() {
        for fence in [
            "",
            "-1",
            "+1",
            "1e3",
            "0x10",
            " 1",
            "1 ",
            "18446744073709551616",
        ] {
            bad_pointer(Some(A), Some(fence));
        }
    }

    #[test]
    fn a_pointer_missing_a_field_is_refused() {
        bad_pointer(Some(A), None);
        bad_pointer(None, Some("0"));
    }

    #[test]
    fn a_pointer_that_is_not_utf8_is_refused() {
        let fields = vec![Some(vec![0xff; 16]), Some(b"0".to_vec())];
        assert!(pointer(KEY, &fields).is_err());
    }

    #[test]
    fn a_list_gives_its_ids_in_order_and_each_once() {
        let entries = [format!("{B}:2"), format!("{A}:1"), format!("{B}:0")];
        let entries: Vec<Vec<u8>> = entries.into_iter().map(String::into_bytes).collect();
        assert_eq!(
            listed(LIST, &entries),
            Ok(vec![B.to_string(), A.to_string()])
        );
    }

    #[test]
    fn an_empty_list_names_its_key() {
        let e = listed(LIST, &[]).unwrap_err();
        assert!(
            e.starts_with(&format!("{LIST}: ")) && e.contains("empty"),
            "{e}"
        );
    }

    #[test]
    fn a_list_entry_without_a_fence_is_refused() {
        bad_entry(A.as_bytes());
        bad_entry(format!("{A}:").as_bytes());
    }

    #[test]
    fn a_list_entry_whose_id_is_not_an_id_is_refused() {
        bad_entry(b"000000000000000A:0");
        bad_entry(b"../../../../etc/:0");
        bad_entry(b":0");
    }

    #[test]
    fn a_list_entry_whose_fence_is_not_decimal_is_refused() {
        bad_entry(format!("{A}:-1").as_bytes());
        bad_entry(format!("{A}:1:2").as_bytes());
        bad_entry(format!("{A}:x").as_bytes());
    }

    #[test]
    fn a_list_entry_that_is_not_utf8_is_refused() {
        bad_entry(&[0xff; 18]);
    }

    /// The script against the local Valkey: each id once, newest first, trimmed to `keep`, and
    /// the hash names the last one.
    #[tokio::test(flavor = "current_thread")]
    async fn the_script_keeps_each_id_once_newest_first_within_keep() {
        let Some(url) = crate::valkey::tests::service() else {
            return;
        };
        let pid = std::process::id();
        let toml = format!("url = \"{url}\"\nkey_prefix = \"meada:test-point-{pid}\"");
        let config: valkey::File = toml::from_str(&toml).unwrap();
        let config = config.validate(std::path::Path::new("/")).unwrap();
        let valkey = Valkey::connect(&config).await.unwrap();
        let (current, retained) = keys(&config);
        valkey
            .query::<i64>(cmd("DEL").arg(&current).arg(&retained))
            .await
            .unwrap();
        let c = "000000000000000c";
        for id in [A, B, A, c] {
            point(&valkey, &config, 2, id, None, 0).await.unwrap();
        }
        let list: Vec<String> = valkey
            .query(cmd("LRANGE").arg(&retained).arg(0).arg(-1))
            .await
            .unwrap();
        assert_eq!(list, [format!("{c}:0"), format!("{A}:0")]);
        let read = read(&valkey, &config, 2).await.unwrap();
        assert_eq!(
            read,
            Pointer {
                id: c.into(),
                fence: 0,
                retained: vec![c.into(), A.into()]
            }
        );
        valkey
            .query::<i64>(cmd("DEL").arg(&current).arg(&retained))
            .await
            .unwrap();
    }

    /// With a token, the script writes only while the lock holds it and the fence is above the
    /// current one's, compared as decimals of any length; a refusal names the key and changes
    /// nothing.
    #[tokio::test(flavor = "current_thread")]
    async fn a_token_write_needs_the_lock_and_a_higher_fence() {
        let Some(url) = crate::valkey::tests::service() else {
            return;
        };
        let pid = std::process::id();
        let toml = format!("url = \"{url}\"\nkey_prefix = \"meada:test-fence-{pid}\"");
        let config: valkey::File = toml::from_str(&toml).unwrap();
        let config = config.validate(std::path::Path::new("/")).unwrap();
        let valkey = Valkey::connect(&config).await.unwrap();
        let (current, retained) = keys(&config);
        let lock = config.key("lock", true);
        let fence_key = config.key("fence", true);
        let del = cmd("DEL").arg(&current).arg(&retained).arg(&lock).clone();
        valkey.query::<i64>(&del).await.unwrap();
        let set = |key: &str, value: &str| cmd("SET").arg(key).arg(value).clone();
        async fn id(valkey: &Valkey, current: &str) -> Option<String> {
            let get = cmd("HGET").arg(current).arg("id").clone();
            valkey.query(&get).await.unwrap()
        }
        let token = "0123456789abcdef0123456789abcdef";

        // No lock, or another token: refused, naming the lock.
        let e = point(&valkey, &config, 2, A, Some(token), 1).await;
        assert_eq!(e, Err(Msg::LockNotHeld(&lock).text()));
        valkey.query::<()>(&set(&lock, "another")).await.unwrap();
        assert!(point(&valkey, &config, 2, A, Some(token), 1).await.is_err());
        assert_eq!(id(&valkey, &current).await, None);

        valkey.query::<()>(&set(&lock, token)).await.unwrap();
        point(&valkey, &config, 2, A, Some(token), 9).await.unwrap();
        let refused = Msg::FenceRefused {
            fence: 9,
            current: &current,
            fence_key: &fence_key,
        }
        .text();
        assert!(refused.contains(&fence_key), "{refused}");
        let e = point(&valkey, &config, 2, B, Some(token), 9).await;
        assert_eq!(e, Err(refused));
        let e = point(&valkey, &config, 2, B, Some(token), 3).await;
        assert!(e.unwrap_err().contains("refused fence 3"));
        assert_eq!(id(&valkey, &current).await.as_deref(), Some(A));
        point(&valkey, &config, 2, B, Some(token), 10)
            .await
            .unwrap();
        assert_eq!(id(&valkey, &current).await.as_deref(), Some(B));

        // Leading zeros and fences past a Lua number's precision compare as decimals.
        let hset = |fence: &str| cmd("HSET").arg(&current).arg("fence").arg(fence).clone();
        valkey.query::<i64>(&hset("011")).await.unwrap();
        assert!(
            point(&valkey, &config, 2, A, Some(token), 11)
                .await
                .is_err()
        );
        point(&valkey, &config, 2, A, Some(token), 12)
            .await
            .unwrap();
        valkey
            .query::<i64>(&hset("18446744073709551614"))
            .await
            .unwrap();
        let near = u64::MAX - 1;
        assert!(
            point(&valkey, &config, 2, B, Some(token), near)
                .await
                .is_err()
        );
        point(&valkey, &config, 2, B, Some(token), u64::MAX)
            .await
            .unwrap();

        // A fence in `current` that is no decimal is refused as a bad pointer.
        valkey.query::<i64>(&hset("1e3")).await.unwrap();
        let e = point(&valkey, &config, 2, A, Some(token), 5).await;
        assert_eq!(e, Err(Msg::GenerationsBadPointer(&current).text()));
        // Without a token nothing is checked, as without the refresh lock.
        point(&valkey, &config, 2, A, None, 0).await.unwrap();
        valkey.query::<i64>(&del).await.unwrap();
    }
}
