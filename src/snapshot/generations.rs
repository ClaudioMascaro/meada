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
    pub fn validate(self) -> Result<Config, (&'static str, String)> {
        let keep = self.keep.unwrap_or(KEEP_DEFAULT);
        match u64::try_from(keep) {
            Ok(keep) if keep >= KEEP_MIN as u64 => Ok(Config { keep }),
            _ => Err(("generations.keep", Msg::GenerationsKeep(KEEP_MIN).text())),
        }
    }
}

/// The pointer's two keys.
pub fn keys(valkey: &valkey::Config) -> (String, String) {
    (valkey.key("current", true), valkey.key("retained", true))
}

/// The pointer script. KEYS: current, retained. ARGV: id, fence, keep, lock token (empty for
/// none). It removes every entry of the id from the list before pushing it, so the list holds
/// each id once, trims the list to `keep` and sets the hash, all at once. #276 checks the token
/// against the lock and the fence against the current one here; without it the fence is `0`
/// and nothing checks it.
const POINT: &str = r"
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
/// last write of `meada snapshot publish`. `token` is the refresh lock's (#276), `None` without
/// one.
pub async fn point(
    valkey: &Valkey,
    config: &valkey::Config,
    keep: u64,
    id: &str,
    token: Option<&str>,
    fence: u64,
) -> Result<(), String> {
    let (current, retained) = keys(config);
    let mut script = cmd("EVAL");
    script.arg(POINT).arg(2).arg(&current).arg(&retained);
    script.arg(id).arg(fence).arg(keep).arg(token.unwrap_or(""));
    let wrote = valkey.query::<i64>(&script).await;
    wrote
        .map(drop)
        .map_err(|e| format!("{current}, {retained}: {e}"))
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
fn pointer(key: &str, fields: &[Option<Vec<u8>>]) -> Result<(String, u64), String> {
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
}
