//! The one Valkey client the cloud modules share (ADR 0012, feature `valkey`), configured by the
//! manifest's `[valkey]` section: TLS outside loopback, an AUTH/RBAC user and password, a
//! standalone server or a cluster, and every key under the manifest's `key_prefix`. The
//! password comes only from the environment variable or the file the section names; it
//! appears in no error and on no stderr line.

use std::path::{Path, PathBuf};
use std::time::Duration;

use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use redis::cluster::ClusterClientBuilder;
use redis::cluster_async::ClusterConnection;
use redis::{
    Client, Cmd, ConnectionAddr, FromRedisValue, IntoConnectionInfo, TlsCertificates, cmd,
};
use serde::Deserialize;

use crate::modules::{Problem, Secret};
use crate::msg::Msg;

/// How long `meada status` waits for Valkey to answer its ping: a dead one must not stall the
/// probe.
const PING_TIMEOUT: Duration = Duration::from_millis(800);
/// A connection attempt, and an answer, of the shared client.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(2);
// ponytail: the most `password_file` may hold; past it the connection fails, naming the file.
const PASSWORD_MAX: u64 = 4096;
// ponytail: the most `ca_file` may hold; past it the connection fails, naming the file.
const CA_MAX: u64 = 1 << 20;

/// The `[valkey]` section as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    url: String,
    password_env: Option<String>,
    password_file: Option<String>,
    #[serde(default)]
    cluster: bool,
    key_prefix: Option<String>,
    ca_file: Option<String>,
}

/// The validated section. The URL holds no password, so nothing here is a secret.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    url: String,
    password: Option<Secret>,
    cluster: bool,
    key_prefix: String,
    ca_file: Option<PathBuf>,
}

impl File {
    /// The section checked; relative paths are relative to `base`, the manifest's directory.
    pub fn validate(self, base: &Path) -> Result<Config, Problem> {
        let url = "valkey.url";
        let Ok(info) = self.url.as_str().into_connection_info() else {
            return Err((url, Msg::ValkeyUrlInvalid.text()));
        };
        if info.redis_settings().password().is_some() {
            return Err((url, Msg::ValkeyUrlPassword.text()));
        }
        match info.addr() {
            ConnectionAddr::Tcp(host, _) if !crate::modules::loopback(host) => {
                return Err((url, Msg::ValkeyUrlPlain.text()));
            }
            ConnectionAddr::TcpTls { insecure: true, .. } => {
                return Err((url, Msg::ValkeyUrlInsecure.text()));
            }
            ConnectionAddr::Unix(_) => return Err((url, Msg::ValkeyUrlInvalid.text())),
            // A cluster announces its nodes, which are not loopback: TLS from the seed on.
            ConnectionAddr::Tcp(..) if self.cluster => {
                return Err(("valkey.cluster", Msg::ValkeyClusterPlain.text()));
            }
            _ => {}
        }
        let both = || ("valkey.password_file", Msg::ValkeyBoth.text());
        let password = Secret::named(self.password_env, self.password_file, base, both)?;
        let key_prefix = self.key_prefix.unwrap_or_else(|| "meada".into());
        let prefix = "valkey.key_prefix";
        if key_prefix.is_empty() {
            return Err((prefix, Msg::Empty.text()));
        }
        if key_prefix.contains(['{', '}']) {
            return Err((prefix, Msg::ValkeyPrefixBraces.text()));
        }
        Ok(Config {
            url: self.url,
            password,
            cluster: self.cluster,
            key_prefix,
            ca_file: self.ca_file.map(|f| base.join(f)),
        })
    }
}

impl Config {
    /// The one function every key the modules build goes through: `<key_prefix>:<name>`, or
    /// `{<key_prefix>}:<name>` when `tagged`, so the keys one script reads or writes together
    /// (the pointer, the lock and the fence) land on one slot in cluster mode.
    pub fn key(&self, name: &str, tagged: bool) -> String {
        let prefix = &self.key_prefix;
        match tagged {
            true => format!("{{{prefix}}}:{name}"),
            false => format!("{prefix}:{name}"),
        }
    }

    /// The AUTH password, read now from the variable or the file the section names (on a
    /// blocking thread, so a slow mount stalls no timeout).
    async fn auth(&self) -> Result<Option<String>, String> {
        let Some(password) = self.password.clone() else {
            return Ok(None);
        };
        let read = tokio::task::spawn_blocking(move || password.read(PASSWORD_MAX)).await;
        match (read.map_err(|e| e.to_string())??, &self.password) {
            (None, Some(Secret::Env(var))) => Err(Msg::ValkeyEnvUnset(var).text()),
            (value, _) => Ok(value),
        }
    }
}

/// `config::read_bounded` on a blocking thread, so a slow mount stalls no timeout.
async fn read_file(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let path = path.to_path_buf();
    let shown = path.display().to_string();
    let read = move || crate::config::read_bounded(&path, max);
    tokio::task::spawn_blocking(read)
        .await
        .unwrap_or(Err(shown))
}

enum Conn {
    Single(ConnectionManager),
    Cluster(ClusterConnection),
}

/// A connection to the configured Valkey, standalone or cluster.
pub struct Valkey {
    conn: Conn,
    secret: Option<String>,
}

impl Valkey {
    pub async fn connect(config: &Config) -> Result<Self, String> {
        let secret = config.auth().await?;
        let secret_text = secret.clone();
        let scrub = |e: redis::RedisError| scrub(&e.to_string(), secret_text.as_deref());
        let mut info = (config.url.as_str().into_connection_info()).map_err(scrub)?;
        if let Some(password) = &secret {
            let settings = info.redis_settings().clone().set_password(password);
            info = info.set_redis_settings(settings);
        }
        let tls = match &config.ca_file {
            Some(file) if matches!(info.addr(), ConnectionAddr::TcpTls { .. }) => {
                let root_cert = read_file(file, CA_MAX).await?;
                Some(TlsCertificates {
                    client_tls: None,
                    root_cert: Some(root_cert),
                })
            }
            _ => None,
        };
        let conn = if config.cluster {
            let mut builder = ClusterClientBuilder::new([info])
                .connection_timeout(NETWORK_TIMEOUT)
                .response_timeout(NETWORK_TIMEOUT);
            if let Some(tls) = tls {
                builder = builder.certs(tls);
            }
            let client = builder.build().map_err(scrub)?;
            Conn::Cluster(client.get_async_connection().await.map_err(scrub)?)
        } else {
            let client = match tls {
                Some(tls) => Client::build_with_tls(info, tls),
                None => Client::open(info),
            }
            .map_err(scrub)?;
            let settings = ConnectionManagerConfig::new()
                .set_connection_timeout(Some(NETWORK_TIMEOUT))
                .set_response_timeout(Some(NETWORK_TIMEOUT))
                .set_number_of_retries(1);
            let manager = ConnectionManager::new_with_config(client, settings);
            Conn::Single(manager.await.map_err(scrub)?)
        };
        Ok(Self { conn, secret })
    }

    /// Runs `command`; an error never carries the password.
    pub async fn query<T: FromRedisValue>(&self, command: &Cmd) -> Result<T, String> {
        let answer = match &self.conn {
            Conn::Single(c) => command.query_async(&mut c.clone()).await,
            Conn::Cluster(c) => command.query_async(&mut c.clone()).await,
        };
        answer.map_err(|e| scrub(&e.to_string(), self.secret.as_deref()))
    }

    pub async fn ping(&self) -> Result<(), String> {
        self.query::<String>(&cmd("PING")).await.map(drop)
    }
}

/// `text` without the secret, if the text carries it.
fn scrub(text: &str, secret: Option<&str>) -> String {
    match secret {
        Some(secret) if !secret.is_empty() => text.replace(secret, "***"),
        _ => text.to_string(),
    }
}

/// Whether Valkey answers within `PING_TIMEOUT`, or why not.
pub fn reach(config: &Config) -> Result<(), String> {
    let ping = async { Valkey::connect(config).await?.ping().await };
    crate::modules::within(PING_TIMEOUT, ping, Msg::ValkeyTimeout)
}

/// The `valkey:` line `meada status --workspace` prints.
pub fn status_line(config: &Config) -> String {
    match reach(config) {
        Ok(()) => Msg::ValkeyReachable.text(),
        Err(reason) => Msg::ValkeyUnreachable(&reason).text(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use super::*;

    fn section(toml: &str) -> Result<Config, Problem> {
        let file: File = toml::from_str(toml).unwrap();
        file.validate(Path::new("/etc/meada"))
    }

    #[test]
    fn the_section_takes_its_defaults_and_resolves_paths_from_the_manifest() {
        let config = section("url = \"redis://localhost\"").unwrap();
        assert_eq!(config.key_prefix, "meada");
        assert_eq!(config.password, None);
        assert!(!config.cluster && config.ca_file.is_none());
        let config = section(
            "url = \"rediss://meada@cache.example.internal:6379\"\npassword_file = \"valkey\"\n\
             cluster = true\nkey_prefix = \"meada:prod\"\nca_file = \"/etc/meada/ca.pem\"",
        )
        .unwrap();
        assert_eq!(
            config.password,
            Some(Secret::File("/etc/meada/valkey".into()))
        );
        assert_eq!(config.ca_file, Some("/etc/meada/ca.pem".into()));
        assert!(config.cluster);
    }

    #[test]
    fn a_bad_section_names_the_key_and_never_the_password() {
        let secret = "hunter2";
        for (toml, key) in [
            (
                "url = \"rediss://u:hunter2@cache.example.internal\"",
                "valkey.url",
            ),
            (
                "url = \"redis://cache.example.internal:6379\"",
                "valkey.url",
            ),
            (
                "url = \"rediss://cache.example.internal/#insecure\"",
                "valkey.url",
            ),
            ("url = \"http://cache.example.internal\"", "valkey.url"),
            (
                "url = \"redis://127.0.0.1\"\npassword_env = \"A\"\npassword_file = \"b\"",
                "valkey.password_file",
            ),
            (
                "url = \"redis://[::1]\"\nkey_prefix = \"a{b\"",
                "valkey.key_prefix",
            ),
            (
                "url = \"redis://[::1]\"\nkey_prefix = \"a}\"",
                "valkey.key_prefix",
            ),
            (
                "url = \"redis://[::1]\"\nkey_prefix = \"\"",
                "valkey.key_prefix",
            ),
        ] {
            let (found, message) = section(toml).unwrap_err();
            assert_eq!(found, key, "{toml}");
            assert!(!message.contains(secret), "{message}");
        }
        // Loopback may stay plain, in any of its forms.
        for host in ["localhost", "127.0.0.1", "127.9.9.9", "[::1]"] {
            let url = format!("url = \"redis://{host}\"");
            assert!(section(&url).is_ok(), "{host}");
        }
    }

    #[test]
    fn a_cluster_needs_a_tls_seed_because_it_announces_its_own_nodes() {
        let (key, _) = section("url = \"redis://127.0.0.1\"\ncluster = true").unwrap_err();
        assert_eq!(key, "valkey.cluster");
        assert!(section("url = \"rediss://127.0.0.1\"\ncluster = true").is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_file_is_read_only_when_it_is_a_regular_file_within_the_bound() {
        let dir = std::env::temp_dir().join(format!("meada-valkey-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (small, big) = (dir.join("small"), dir.join("big"));
        std::fs::write(&small, "abc\n").unwrap();
        std::fs::write(&big, vec![b'x'; 4097]).unwrap();
        assert_eq!(read_file(&small, PASSWORD_MAX).await.unwrap(), b"abc\n");
        for (path, why) in [
            (&dir, "not a regular file"),
            (&big, "larger than 4096 bytes"),
        ] {
            let err = read_file(path, PASSWORD_MAX).await.unwrap_err();
            assert!(err.ends_with(why), "{err}");
        }
        assert!(read_file(&dir.join("none"), PASSWORD_MAX).await.is_err());
        let ca = dir.join("ca");
        std::fs::write(&ca, vec![b'x'; CA_MAX as usize + 1]).unwrap();
        let err = read_file(&ca, CA_MAX).await.unwrap_err();
        assert!(err.ends_with("larger than 1048576 bytes"), "{err}");
    }

    #[test]
    fn every_key_goes_through_the_prefix_and_the_tagged_ones_share_a_hash_tag() {
        let config = section("url = \"redis://localhost\"\nkey_prefix = \"meada:prod\"").unwrap();
        assert_eq!(config.key("note:abc", false), "meada:prod:note:abc");
        assert_eq!(config.key("current", true), "{meada:prod}:current");
        assert_eq!(config.key("fence", true), "{meada:prod}:fence");
    }

    /// The local Valkey the tests name, or `None` (with the reason printed) when the variable is
    /// unset; `MEADA_TEST_SERVICES_REQUIRED=1` turns that into a failure.
    pub(crate) fn service() -> Option<String> {
        let url = std::env::var("MEADA_TEST_VALKEY_URL").ok();
        if url.is_none() {
            let required = std::env::var("MEADA_TEST_SERVICES_REQUIRED").is_ok_and(|v| v == "1");
            assert!(!required, "MEADA_TEST_VALKEY_URL is not set");
            eprintln!("skipped: MEADA_TEST_VALKEY_URL is not set");
        }
        url
    }

    /// A password made at run time, another on each call (each `RandomState` has keys of its
    /// own): the tests use no literal credential.
    fn made_password() -> String {
        use std::hash::{BuildHasher, RandomState};
        RandomState::new().hash_one(std::process::id()).to_string()
    }

    /// A config for `url` as `user`, the password in a file of this test's own.
    fn with_password(url: &str, user: &str, password: &str, name: &str) -> Config {
        let pid = std::process::id();
        let file = std::env::temp_dir().join(format!("meada-valkey-{pid}-{name}"));
        std::fs::write(&file, format!("{password}\n")).unwrap();
        let (scheme, rest) = url.split_once("://").unwrap();
        let toml = format!(
            "url = \"{scheme}://{user}@{rest}\"\npassword_file = \"{}\"\n\
             key_prefix = \"meada:test-{pid}\"",
            file.display()
        );
        section(&toml).unwrap()
    }

    async fn admin(url: &str) -> redis::aio::MultiplexedConnection {
        // The defaults (1 s to connect, 500 ms per response) fail under load.
        let config = redis::AsyncConnectionConfig::new()
            .set_connection_timeout(Some(NETWORK_TIMEOUT))
            .set_response_timeout(Some(NETWORK_TIMEOUT));
        let client = Client::open(url).unwrap();
        let connected = client.get_multiplexed_async_connection_with_config(&config);
        (connected.await).unwrap_or_else(|e| panic!("the admin connection to {url}: {e}"))
    }

    /// A TLS handshake with a port that closes every connection fails with an error. In a build
    /// with every module, two rustls providers made it panic instead (one provider, ring, now).
    #[tokio::test(flavor = "current_thread")]
    async fn a_tls_handshake_that_fails_is_an_error_and_no_panic() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                drop(stream);
            }
        });
        let config = section(&format!("url = \"rediss://127.0.0.1:{port}\"")).unwrap();
        assert!(Valkey::connect(&config).await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ping_and_a_value_go_through_the_prefix() {
        let Some(url) = service() else { return };
        let pid = std::process::id();
        let config = section(&format!(
            "url = \"{url}\"\nkey_prefix = \"meada:test-{pid}\""
        ))
        .unwrap();
        let valkey = Valkey::connect(&config).await.unwrap();
        valkey.ping().await.unwrap();
        let key = config.key("probe", false);
        let set = cmd("SET").arg(&key).arg("v").clone();
        valkey.query::<()>(&set).await.unwrap();
        // Read back by the literal key, not by the function that built it.
        let literal = format!("meada:test-{pid}:probe");
        let mut root = admin(&url).await;
        let got: String = cmd("GET")
            .arg(&literal)
            .query_async(&mut root)
            .await
            .unwrap();
        assert_eq!(got, "v");
        cmd("DEL")
            .arg(&literal)
            .exec_async(&mut root)
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_acl_user_connects_and_a_wrong_password_is_refused_without_echoing_it() {
        let Some(url) = service() else { return };
        let user = format!("meada-test-{}", std::process::id());
        let secret = &made_password();
        let mut root = admin(&url).await;
        let mut setup = cmd("ACL");
        setup
            .arg("SETUSER")
            .arg(&user)
            .arg("on")
            .arg(format!(">{secret}"));
        setup.arg("~*").arg("&*").arg("+@all");
        setup.exec_async(&mut root).await.unwrap();

        let good = with_password(&url, &user, secret, "good");
        let valkey = Valkey::connect(&good).await.unwrap();
        let who: String = valkey.query(cmd("ACL").arg("WHOAMI")).await.unwrap();
        assert_eq!(who, user);

        let wrong = &made_password();
        let bad = with_password(&url, &user, wrong, "bad");
        let reason = Valkey::connect(&bad)
            .await
            .err()
            .expect("a wrong password is refused");
        assert!(!reason.is_empty(), "{reason}");
        assert!(
            !reason.contains(wrong) && !reason.contains(secret),
            "{reason}"
        );

        // TLS to a plain port fails, and says nothing of the password either.
        let tls = with_password(
            &url.replacen("redis://", "rediss://", 1),
            &user,
            secret,
            "tls",
        );
        let reason = Valkey::connect(&tls)
            .await
            .err()
            .expect("TLS to a plain port fails");
        assert!(!reason.is_empty() && !reason.contains(secret), "{reason}");
        cmd("ACL")
            .arg("DELUSER")
            .arg(&user)
            .exec_async(&mut root)
            .await
            .unwrap();
    }
}
