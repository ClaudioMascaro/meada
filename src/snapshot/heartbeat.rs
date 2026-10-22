//! The node heartbeat (ADR 0014, #298, feature `generation-pointer`): with `[generations]`,
//! every round of `meada snapshot pull --every <seconds>` ends by writing the node's state into
//! Valkey, whether the round moved the pointer, changed nothing or failed. A node gains no HTTP
//! surface: the console reads these keys, under the pointer's hash tag.
//!
//! `{<key_prefix>}:node:<name>` holds a JSON document: `name`, `generation` and `fence` (the
//! node pointer file's, null without one), `held` (the generation ids under `local_dir`,
//! sorted), `ready` (the predicate `meada status --generations` exits on), `version` and `time`
//! (RFC 3339, UTC, to the second). Its TTL is three periods. `{<key_prefix>}:nodes` is a sorted
//! set of the names scored by the expiry in Unix seconds; the script that writes a heartbeat
//! also removes every name whose score has passed. A Valkey that cannot be reached within
//! `WRITE_TIMEOUT` costs one warning for the round and changes nothing else.

use std::path::Path;
use std::time::Duration;

use redis::cmd;

use super::generations::{now, rfc3339};
use super::{Config, is_node_name, pull, serve};
use crate::msg::Msg;
use crate::valkey::{self, Valkey};
use crate::workspace::Manifest;
use crate::{Result, engine};

/// How many periods a heartbeat lives: a node that misses two rounds disappears.
const PERIODS: u64 = 3;
/// How long the write waits for Valkey, as `status --generations` does: an unreachable Valkey
/// never delays the next round by more.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// The write. KEYS: the heartbeat, the node set. ARGV: the document, the TTL in seconds, the
/// name. The score is the expiry by Valkey's own clock, so no node's skew removes another's
/// name; names whose score is at or below now are removed.
const WRITE: &str = r"
local now = tonumber(redis.call('TIME')[1])
redis.call('SET', KEYS[1], ARGV[1], 'EX', ARGV[2])
redis.call('ZADD', KEYS[2], now + tonumber(ARGV[2]), ARGV[3])
redis.call('ZREMRANGEBYSCORE', KEYS[2], '-inf', now)
return 1
";

/// The host name in lowercase, as a holder or a heartbeat names it.
pub fn host() -> String {
    #[cfg(unix)]
    let name = nix::unistd::gethostname()
        .ok()
        .and_then(|h| h.into_string().ok());
    #[cfg(not(unix))]
    let name = std::env::var("COMPUTERNAME").ok();
    name.unwrap_or_default().to_lowercase()
}

/// The node's name: `[snapshot] node_name`, else the host name, which must be a node name too.
fn name(config: &Config) -> std::result::Result<String, String> {
    resolve(config.node_name.as_deref(), &host())
}

fn resolve(configured: Option<&str>, host: &str) -> std::result::Result<String, String> {
    match (configured, is_node_name(host)) {
        (Some(name), _) => Ok(name.to_string()),
        (None, true) => Ok(host.to_string()),
        (None, false) => Err(Msg::NodeNameHost(&host.escape_debug().to_string()).text()),
    }
}

/// With `[generations]`, the error that keeps `pull --every` from starting: the host name is no
/// node name and `[snapshot] node_name` is not set. An unreadable manifest is the pull's error.
pub fn check(manifest: &Path) -> Result<()> {
    if let Ok(Manifest {
        snapshot: Some(config),
        generations: Some(_),
        ..
    }) = Manifest::read(manifest)
    {
        name(&config)?;
    }
    Ok(())
}

/// The round's heartbeat, written when the manifest has `[generations]`; `every` is the period
/// in seconds. The warning for the round when it could not be written (never with a password).
pub fn beat(manifest: &Path, every: u64) -> Option<String> {
    let Ok(Manifest {
        snapshot: Some(config),
        generations: Some(_),
        valkey: Some(valkey),
        ..
    }) = Manifest::read(manifest)
    else {
        return None;
    };
    let name = match name(&config) {
        Ok(name) => name,
        Err(warning) => return Some(warning),
    };
    let key = valkey.key(&format!("node:{name}"), true);
    let local = config.local_dir.as_deref()?;
    let document = document(manifest, local, &name).to_string();
    let put = put(&valkey, &key, &document, every * PERIODS, &name);
    let written = crate::modules::within(WRITE_TIMEOUT, put, Msg::ValkeyTimeout);
    written.err().map(|reason| {
        Msg::HeartbeatNotWritten {
            key: &key,
            reason: &reason,
        }
        .text()
    })
}

/// The heartbeat document of the node at `local`, as the files say now.
fn document(manifest: &Path, local: &Path, name: &str) -> serde_json::Value {
    let pointer = pull::current(local).ok().flatten();
    let ready = crate::index::with_busy_timeout(engine::PROBE_BUSY, || {
        engine::served_generation(manifest).is_ok()
    });
    serde_json::json!({
        "name": name,
        "generation": pointer.as_ref().map(|(id, _)| id),
        "fence": pointer.as_ref().map(|(_, fence)| fence),
        "held": serve::ids(local),
        "ready": ready,
        "version": env!("CARGO_PKG_VERSION"),
        "time": rfc3339(now()),
    })
}

/// Runs `WRITE` for the heartbeat `document` at `key`, living `ttl` seconds.
async fn put(
    config: &valkey::Config,
    key: &str,
    document: &str,
    ttl: u64,
    name: &str,
) -> std::result::Result<(), String> {
    let valkey = Valkey::connect(config).await?;
    let mut script = cmd("EVAL");
    script
        .arg(WRITE)
        .arg(2)
        .arg(key)
        .arg(config.key("nodes", true));
    script.arg(document).arg(ttl).arg(name);
    valkey.query::<i64>(&script).await.map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_configured_name_wins_and_a_host_name_must_be_a_node_name() {
        let named = |name, host| resolve(name, host);
        assert_eq!(
            named(Some("meada-node-0"), "Host_1").unwrap(),
            "meada-node-0"
        );
        assert_eq!(named(None, "meada-node-1").unwrap(), "meada-node-1");
        for host in ["", "Host", "a_b", &"a".repeat(64)] {
            let refused = named(None, host).unwrap_err();
            assert!(
                refused.contains("[snapshot] node_name"),
                "{host}: {refused}"
            );
        }
    }
}
