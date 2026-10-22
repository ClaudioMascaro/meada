//! The refresh lock (ADR 0012, feature `refresh-lock`, section `[refresh_lock]`): `meada
//! snapshot lock --workspace <manifest> -- <command>` takes the workspace's lock in Valkey, runs
//! the operator's whole refresh as its child and releases the lock. Each acquisition gets a
//! fencing token, which the child's `meada snapshot publish` hands to the pointer script
//! (`generations::point`), so an overlapping or stalled job never replaces a newer generation
//! with an older one.
//!
//! Keys, under the `[valkey]` key prefix and the pointer's hash tag: `{<key_prefix>}:lock`
//! (`SET NX PX`, renewed every third of the TTL by compare-and-extend, released by
//! compare-and-delete) and `{<key_prefix>}:fence` (`INCR` per acquisition, never lowered or
//! deleted by Meada).
//!
//! Supervision: the child runs in a process group of its own, with the token and the fence in
//! `MEADA_REFRESH_LOCK` and `MEADA_REFRESH_FENCE`. When a renewal finds another token, or none
//! succeeded for a whole TTL, the lock is lost: the group gets SIGTERM, then SIGKILL after
//! `KILL_GRACE`, and `lock` exits 1. A SIGTERM, SIGINT, SIGHUP or SIGQUIT to `lock` is passed to
//! the group the same way. Once the child exits, whatever it left in its group gets SIGKILL, so
//! nothing is left in its process group (a grandchild that calls `setsid` leaves it, and
//! escapes); on Linux the child also gets SIGTERM if `lock` dies first. Unix only.

use std::ffi::OsString;
use std::future::Future;
use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::pin::Pin;
use std::process::Command;
use std::time::Duration;

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use redis::cmd;
use serde::Deserialize;
use tokio::signal::unix::{SignalKind, signal};
use tokio::time::{Instant, sleep_until, timeout_at};

use crate::msg::Msg;
use crate::valkey::{self, Valkey};
use crate::workspace::Manifest;

/// The variables the child gets: the lock's token and the acquisition's fence.
pub const TOKEN_ENV: &str = "MEADA_REFRESH_LOCK";
pub const FENCE_ENV: &str = "MEADA_REFRESH_FENCE";
/// The shortest `ttl_seconds`, and its default.
const TTL_MIN: i64 = 10;
const TTL_DEFAULT: i64 = 300;
/// How long the child's group has to exit after SIGTERM before it gets SIGKILL.
const KILL_GRACE: Duration = Duration::from_secs(10);
/// A release's wait for Valkey: the TTL frees a lock a release could not.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(2);

/// The `[refresh_lock]` section as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    ttl_seconds: Option<i64>,
}

/// The validated section.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// How long the lock lives without a renewal, at least `TTL_MIN` seconds.
    pub ttl: Duration,
}

impl File {
    pub fn validate(self) -> Result<Config, crate::modules::Problem> {
        let key = "refresh_lock.ttl_seconds";
        let ttl = crate::modules::at_least(key, self.ttl_seconds, TTL_DEFAULT, TTL_MIN)?;
        Ok(Config {
            ttl: Duration::from_secs(ttl),
        })
    }
}

/// The lock's keys: the lock, the fence and the holder.
pub fn keys(config: &valkey::Config) -> (String, String, String) {
    let key = |name| config.key(name, true);
    (key("lock"), key("fence"), key("holder"))
}

/// The token and fence of the lock a job runs under, as `publish` reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub token: String,
    pub fence: u64,
}

/// What `meada snapshot lock` handed its child: both variables, the token of 32 lowercase hex
/// characters and the fence a positive decimal, or the error naming them.
pub fn from_env() -> Result<Held, String> {
    let token = std::env::var(TOKEN_ENV).ok().filter(|t| is_token(t));
    let fence = (std::env::var(FENCE_ENV).ok())
        .and_then(|f| super::fence(&f))
        .filter(|f| *f > 0);
    match (token, fence) {
        (Some(token), Some(fence)) => Ok(Held { token, fence }),
        _ => Err(Msg::LockEnv.text()),
    }
}

fn is_token(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A fresh token: 16 random bytes in hex.
fn token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    read.map_err(|e| format!("/dev/urandom: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether `{<key_prefix>}:lock` still holds `token`: what `publish` checks before each upload.
pub async fn holds(valkey: &Valkey, config: &valkey::Config, token: &str) -> Result<(), String> {
    let lock = keys(config).0;
    let get = cmd("GET").arg(&lock).clone();
    let held: Option<Vec<u8>> = valkey
        .query(&get)
        .await
        .map_err(|e| format!("{lock}: {e}"))?;
    match held.as_deref() == Some(token.as_bytes()) {
        true => Ok(()),
        false => Err(Msg::LockNotHeld(&lock).text()),
    }
}

/// Takes the lock and writes its holder, both with the TTL. KEYS: lock, holder. ARGV: token, TTL
/// in ms, then the holder's host, pid, started and holds. Nil when taken; else the holder's four
/// fields, each nil when missing (a lock taken by an older binary has no holder).
const TAKE: &str = r"
if redis.call('SET', KEYS[1], ARGV[1], 'NX', 'PX', ARGV[2]) then
  redis.call('DEL', KEYS[2])
  redis.call('HSET', KEYS[2], 'host', ARGV[3], 'pid', ARGV[4], 'started', ARGV[5], 'holds', ARGV[6])
  redis.call('PEXPIRE', KEYS[2], ARGV[2])
  return false
end
return redis.call('HMGET', KEYS[2], 'host', 'pid', 'started', 'holds')
";

/// Compare-and-extend, the holder with the lock (KEYS: lock, holder): 1 when the lock held the
/// token and lives `ARGV[2]` ms more, 0 if not.
const RENEW: &str = r"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  redis.call('PEXPIRE', KEYS[2], ARGV[2])
  return redis.call('PEXPIRE', KEYS[1], ARGV[2])
end
return 0
";

/// Compare-and-delete: the lock and its holder go only while the lock holds the token.
const RELEASE: &str = r"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  redis.call('DEL', KEYS[2])
  return redis.call('DEL', KEYS[1])
end
return 0
";

/// Takes the refresh lock for `ttl` as `meada snapshot lock` does, its holder saying what
/// `holds` it, then `INCR`s the fence: the token and the fence, or an error naming the key (a
/// lock already held names its holder). Whoever gets `Ok` calls `release`.
pub async fn acquire(
    valkey: &Valkey,
    config: &valkey::Config,
    ttl: Duration,
    holds: &str,
) -> Result<Held, String> {
    let (lock, fence_key, holder) = keys(config);
    let token = token()?;
    let mut take = cmd("EVAL");
    take.arg(TAKE).arg(2).arg(&lock).arg(&holder);
    take.arg(&token).arg(millis(ttl)).arg(host());
    take.arg(std::process::id()).arg(rfc3339(now())).arg(holds);
    let held: Option<Vec<Option<Vec<u8>>>> = match valkey.query(&take).await {
        Ok(held) => held,
        // The server may have taken the lock and the answer been lost: release ours, if it is.
        Err(e) => {
            release(valkey, config, &token).await;
            return Err(format!("{lock}: {e}"));
        }
    };
    if let Some(fields) = held {
        let fields: Vec<String> = fields.iter().flatten().map(|f| shown(f)).collect();
        let holder = match fields.as_slice() {
            [host, pid, started, holds] => Some([host.as_str(), pid, started, holds]),
            _ => None,
        };
        return Err(Msg::LockHeld { key: &lock, holder }.text());
    }
    let incr = cmd("INCR").arg(&fence_key).clone();
    let fence = match valkey.query::<i64>(&incr).await {
        Ok(fence) => u64::try_from(fence).ok().filter(|f| *f > 0),
        Err(e) => {
            release(valkey, config, &token).await;
            return Err(format!("{fence_key}: {e}"));
        }
    };
    match fence {
        Some(fence) => Ok(Held { token, fence }),
        None => {
            release(valkey, config, &token).await;
            Err(Msg::LockBadFence(&fence_key).text())
        }
    }
}

/// A holder's field as a message shows it: Valkey's bytes are hostile, so none reaches a
/// terminal unescaped.
fn shown(field: &[u8]) -> String {
    String::from_utf8_lossy(field).escape_debug().to_string()
}

/// The host name in lowercase, as the holder names it.
// ponytail: `[snapshot] node_name` (#298) replaces it when the manifest sets one.
pub fn host() -> String {
    let name = nix::unistd::gethostname()
        .ok()
        .and_then(|h| h.into_string().ok());
    name.unwrap_or_default().to_lowercase()
}

/// The author of an operation run from the command line: `cli:<login>@<host>`.
pub fn cli_author() -> String {
    let uid = nix::unistd::getuid();
    let user = nix::unistd::User::from_uid(uid).ok().flatten();
    let login = user.map_or_else(|| uid.to_string(), |u| u.name);
    format!("cli:{login}@{}", host())
}

/// The seconds since the Unix epoch.
pub(super) fn now() -> u64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |d| d.as_secs())
}

/// `secs` since the epoch in RFC 3339, UTC, to the second (the civil date by Howard Hinnant's
/// algorithm, years starting in March).
fn rfc3339(secs: u64) -> String {
    let (days, day) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, min, s) = (day / 3600, day % 3600 / 60, day % 60);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

/// `meada snapshot lock --workspace <manifest> -- <command>`: the child's exit code (128 plus
/// the signal when a signal ended it), or an error: the lock already held (nothing run), the
/// lock lost (the child terminated), Valkey unreachable.
pub fn run(manifest: &Path, command: &[OsString]) -> crate::Result<i32> {
    let listed = Manifest::read(manifest)?;
    let shown = manifest.display().to_string();
    let (Some(section), Some(valkey)) = (listed.refresh_lock, listed.valkey) else {
        return Err(Msg::LockNoSection(&shown).text().into());
    };
    Ok(crate::modules::block_on(supervise(
        &section, &valkey, command,
    ))??)
}

/// The TTL in milliseconds, as `PX` and `PEXPIRE` take it.
fn millis(ttl: Duration) -> u64 {
    u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX)
}

async fn supervise(
    section: &Config,
    config: &valkey::Config,
    command: &[OsString],
) -> Result<i32, String> {
    // Before the child exists: a signal to `lock` from now on reaches its group.
    let listen = |kind| signal(kind).map_err(|e| e.to_string());
    let (mut term, mut int, mut hup, mut quit) = (
        listen(SignalKind::terminate())?,
        listen(SignalKind::interrupt())?,
        listen(SignalKind::hangup())?,
        listen(SignalKind::quit())?,
    );
    let valkey = Valkey::connect(config).await?;
    let (lock, ms) = (keys(config).0, millis(section.ttl));
    let ttl = section.ttl;
    // A renewal counts from before it was sent: the lock lives at least a TTL from then.
    let mut renewed = Instant::now();
    let program = command[0].to_string_lossy();
    let Held { token, fence } = acquire(&valkey, config, ttl, &format!("lock {program}")).await?;
    let child = match spawn(command, &token, fence) {
        Ok(child) => child,
        Err(e) => {
            release(&valkey, config, &token).await;
            return Err(e);
        }
    };
    let group = Pid::from_raw(child.id() as i32);
    let (tx, mut exited) = tokio::sync::oneshot::channel();
    let mut child = child;
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });

    let period = ttl / 3;
    let mut next = renewed + period;
    let (mut lost, mut kill_at): (bool, Option<Instant>) = (false, None);
    let stop = |kill_at: &mut Option<Instant>| {
        if kill_at.is_none() {
            let _ = killpg(group, Signal::SIGTERM);
            *kill_at = Some(Instant::now() + KILL_GRACE);
        }
    };
    // The renewal in flight, polled as a branch of its own, so a signal, the child's exit or
    // the SIGKILL timer never waits for Valkey: when it was sent, and the answer.
    // The command outlives the future that borrows it: declared first, dropped last.
    let renew = cmd("EVAL")
        .arg(RENEW)
        .arg(2)
        .arg(&lock)
        .arg(keys(config).2)
        .arg(&token)
        .arg(ms)
        .clone();
    let (shared, renew) = (&valkey, &renew);
    type Renewal<'a> = Pin<Box<dyn Future<Output = (Instant, Option<i64>)> + 'a>>;
    let mut renewing: Option<Renewal> = None;
    let status = loop {
        let deadline = renewed + ttl;
        tokio::select! {
            status = &mut exited => break status,
            _ = term.recv() => stop(&mut kill_at),
            _ = int.recv() => stop(&mut kill_at),
            _ = hup.recv() => stop(&mut kill_at),
            _ = quit.recv() => stop(&mut kill_at),
            _ = sleep_until(kill_at.unwrap_or(deadline)), if kill_at.is_some() => {
                let _ = killpg(group, Signal::SIGKILL);
                kill_at = Some(Instant::now() + KILL_GRACE);
            }
            _ = sleep_until(deadline), if !lost => {
                lost = true;
                renewing = None;
                stop(&mut kill_at);
            }
            _ = sleep_until(next), if !lost && renewing.is_none() => {
                let sent = Instant::now();
                // A renewal that stalls past the deadline is one that did not succeed.
                let bound = deadline.min(sent + period);
                renewing = Some(Box::pin(async move {
                    let answer = timeout_at(bound, shared.query::<i64>(renew)).await;
                    (sent, answer.ok().and_then(Result::ok))
                }));
            }
            (sent, answer) = async {
                match renewing.as_mut() {
                    Some(renewal) => renewal.await,
                    None => std::future::pending().await,
                }
            }, if renewing.is_some() => {
                renewing = None;
                match answer {
                    Some(1) => renewed = sent,
                    Some(_) => {
                        lost = true;
                        stop(&mut kill_at);
                    }
                    // Failed or stalled: the deadline decides.
                    None => {}
                }
                next = sent + period;
            }
        }
    };
    // Whatever the child left in its group goes with it.
    let _ = killpg(group, Signal::SIGKILL);
    release(&valkey, config, &token).await;
    let status = (status.map_err(|_| "the child's waiter stopped".to_string())?)
        .map_err(|e| e.to_string())?;
    if lost {
        return Err(Msg::LockLost(&lock).text());
    }
    Ok(status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)))
}

/// The child: `command` in a process group of its own, with the token and the fence.
fn spawn(command: &[OsString], token: &str, fence: u64) -> Result<std::process::Child, String> {
    let program = &command[0];
    let mut child = Command::new(program);
    child.args(&command[1..]).env(TOKEN_ENV, token);
    child.env(FENCE_ENV, fence.to_string()).process_group(0);
    #[cfg(target_os = "linux")]
    {
        let parent = nix::unistd::getpid();
        // SAFETY: the closure runs in the forked child before `exec` and calls only `prctl` and
        // `getppid`, which are async-signal-safe, and it allocates nothing (an `Errno` converts
        // to an `io::Error` without a box).
        unsafe {
            child.pre_exec(move || {
                nix::sys::prctl::set_pdeathsig(Signal::SIGTERM)?;
                // `lock` died before the signal was armed: run nothing.
                if nix::unistd::getppid() != parent {
                    return Err(nix::errno::Errno::ESRCH.into());
                }
                Ok(())
            });
        }
    }
    child
        .spawn()
        .map_err(|e| format!("{}: {e}", program.to_string_lossy()))
}

/// Compare-and-delete of the lock and its holder within `RELEASE_TIMEOUT`; a lock it could not
/// release expires with its TTL, the holder with it.
pub async fn release(valkey: &Valkey, config: &valkey::Config, token: &str) {
    let (lock, _, holder) = keys(config);
    let mut script = cmd("EVAL");
    script.arg(RELEASE).arg(2).arg(lock).arg(holder).arg(token);
    let released = valkey.query::<i64>(&script);
    let _ = tokio::time::timeout(RELEASE_TIMEOUT, released).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_defaults_to_five_minutes_and_refuses_less_than_ten_seconds() {
        let section = |toml: &str| toml::from_str::<File>(toml).unwrap().validate();
        assert_eq!(section("").unwrap().ttl, Duration::from_secs(300));
        assert_eq!(
            section("ttl_seconds = 10").unwrap().ttl,
            Duration::from_secs(10)
        );
        for toml in ["ttl_seconds = 9", "ttl_seconds = 0", "ttl_seconds = -30"] {
            let (key, message) = section(toml).unwrap_err();
            assert_eq!(
                (key, message.as_str()),
                ("refresh_lock.ttl_seconds", "must be at least 10")
            );
        }
        assert!(toml::from_str::<File>("ttl = 30").is_err());
    }

    #[test]
    fn a_token_is_32_lowercase_hex_characters_and_never_the_same() {
        let (a, b) = (token().unwrap(), token().unwrap());
        assert!(is_token(&a) && is_token(&b), "{a} {b}");
        assert_ne!(a, b);
        for bad in ["", "0123456789ABCDEF0123456789abcdef", "0123456789abcdef"] {
            assert!(!is_token(bad), "{bad}");
        }
    }

    #[test]
    fn the_keys_share_the_pointer_s_hash_tag() {
        let config: valkey::File =
            toml::from_str("url = \"redis://127.0.0.1\"\nkey_prefix = \"meada:prod\"").unwrap();
        let config = config.validate(Path::new("/")).unwrap();
        assert_eq!(
            keys(&config),
            (
                "{meada:prod}:lock".into(),
                "{meada:prod}:fence".into(),
                "{meada:prod}:holder".into()
            )
        );
    }

    #[test]
    fn a_start_time_is_rfc_3339_in_utc_to_the_second() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_825_599), "2000-02-29T11:59:59Z");
        assert_eq!(rfc3339(1_792_627_199), "2026-10-21T23:59:59Z");
        assert_eq!(rfc3339(4_107_542_400), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn a_holder_s_field_reaches_no_terminal_unescaped() {
        assert_eq!(shown(b"lock sh"), "lock sh");
        assert_eq!(shown(b"a\x1b[2Jb\n"), "a\\u{1b}[2Jb\\n");
    }
}
