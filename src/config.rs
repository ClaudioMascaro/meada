use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

pub use meada_core::manifest::{
    BaseUrl, ChannelClient, HelmEnv, Resource, Serve, declared, gateway_path, line_of,
};

use crate::Result;

#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, rename = "peer")]
    pub peers: Vec<Peer>,
    #[serde(default, deserialize_with = "declared")]
    pub serve: Serve,
    #[serde(default, rename = "resource")]
    pub resources: Vec<Resource>,
    /// Keeps the routes whose handler sits in a test file as providers (`http::is_test_file`):
    /// by default a test's router or fake upstream is no provider of the repository.
    #[serde(default)]
    pub test_routes: bool,
    #[serde(default)]
    pub(crate) iac: crate::iac::mapping::Config,
    /// Base URL declarations (`[[base_url]]`): which directory of this repository a base URL
    /// variable reaches. No variable is in two of them (`read_config_up_to`).
    #[serde(default, rename = "base_url")]
    pub base_urls: Vec<BaseUrl>,
    /// Chart environment declarations (`[[helm_env]]`): where a Helm chart's values keep its
    /// container's environment. No key is in two of them (`read_config_up_to`).
    #[serde(default)]
    pub helm_env: Vec<HelmEnv>,
    /// Client declarations (`[[channel_client]]`): calls that produce or consume a channel. No
    /// `call` is in two of them (`read_config_up_to`).
    #[serde(default, rename = "channel_client")]
    pub channel_clients: Vec<ChannelClient>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    /// The peer's repository identity.
    pub repo: String,
    /// Where the peer is usually cloned, relative to this repository's root.
    pub path: Option<String>,
    /// Variables this repository reads the peer's base URL from. Meada never reads their values.
    #[serde(default)]
    pub base_url_env: Vec<String>,
}

impl Config {
    pub fn peers_reading<'a>(&'a self, var: &'a str) -> impl Iterator<Item = &'a str> {
        let reads = move |p: &&Peer| p.base_url_env.iter().any(|v| v == var);
        self.peers.iter().filter(reads).map(|p| p.repo.as_str())
    }

    pub fn declared_dir(&self, var: &str) -> Option<&str> {
        let declares = |b: &&BaseUrl| b.env.iter().any(|v| v == var);
        self.base_urls.iter().find(declares).map(|b| b.dir.as_str())
    }
}

pub fn load(root: &Path) -> Result<Config> {
    Ok(load_own(root)?.unwrap_or_default())
}

pub fn load_own(root: &Path) -> Result<Option<Config>> {
    read_config_up_to(root, None)
}

pub(crate) fn read_config_up_to(root: &Path, max: Option<u64>) -> Result<Option<Config>> {
    let file = root.join("meada.toml");
    let Some(src) = text(&file, max)? else {
        return Ok(None);
    };
    let config: Config = parse(&file, &src, "meada.toml")?;
    for (table, key, declared) in [
        ("base_url", "env", !config.base_urls.is_empty()),
        ("helm_env", "key", !config.helm_env.is_empty()),
        ("channel_client", "call", !config.channel_clients.is_empty()),
    ] {
        if declared {
            listed_twice(&src, &file, table, key)?;
        }
    }
    Ok(Some(config))
}

fn listed_twice(src: &str, file: &Path, table: &str, key: &str) -> Result<()> {
    use toml::de::{DeTable, DeValue};
    let Ok(parsed) = DeTable::parse(src) else {
        return Ok(());
    };
    let mut seen = std::collections::HashSet::new();
    let entries = (parsed.get_ref().iter()).filter(|(k, _)| k.get_ref() == table);
    for (_, value) in entries {
        let DeValue::Array(entries) = value.get_ref() else {
            continue;
        };
        for entry in entries.iter() {
            let DeValue::Table(entry) = entry.get_ref() else {
                continue;
            };
            for (_, written) in entry.iter().filter(|(k, _)| k.get_ref() == key) {
                let values = match written.get_ref() {
                    DeValue::Array(values) => values.iter().collect(),
                    _ => vec![written],
                };
                for value in values {
                    let DeValue::String(text) = value.get_ref() else {
                        continue;
                    };
                    if !seen.insert(text.to_string()) {
                        let twice = meada_core::msg::Msg::ListedTwice.text();
                        let line = line_of(src, value.span().start);
                        let at = format!("{}:{line}", file.display());
                        return Err(format!("{at}: `{table}.{key}`: `{text}` {twice}").into());
                    }
                }
            }
        }
    }
    Ok(())
}

// ponytail: a flat 1 MiB, as `lang::MAX_BYTES`; real ones are a few kilobytes.
pub(crate) const GENERATION_CONFIG_MAX: u64 = 1 << 20;

pub(crate) fn read_own_up_to<T: DeserializeOwned>(
    root: &Path,
    name: &str,
    max: Option<u64>,
) -> Result<Option<T>> {
    let file = root.join(name);
    match text(&file, max)? {
        Some(src) => parse(&file, &src, name).map(Some),
        None => Ok(None),
    }
}

pub(crate) fn text(file: &Path, max: Option<u64>) -> Result<Option<String>> {
    let missing = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    let Some(max) = max else {
        return match fs::read_to_string(file) {
            Ok(src) => Ok(Some(src)),
            Err(e) if missing(&e) => Ok(None),
            Err(e) => Err(format!("{}: {e}", file.display()).into()),
        };
    };
    if fs::symlink_metadata(file).is_err_and(|e| missing(&e)) {
        return Ok(None);
    }
    let bytes = read_unlinked(file, max)?;
    let src = String::from_utf8(bytes)
        .map_err(|_| format!("{}: {}", file.display(), std::io::ErrorKind::InvalidData))?;
    Ok(Some(src))
}

/// `src`, the content of `file`, parsed as `T`; an error names the file, the line and the key,
/// `name` being the kind of file the key belongs to (`meada.toml`, `meada.workspace.toml`).
pub(crate) fn parse<T: DeserializeOwned>(file: &Path, src: &str, name: &str) -> Result<T> {
    Ok(meada_core::manifest::parse(
        &file.display().to_string(),
        src,
        name,
    )?)
}

pub(crate) fn error_at(
    file: &Path,
    src: &str,
    key: &str,
    message: &str,
) -> Box<dyn std::error::Error> {
    meada_core::manifest::error_at(&file.display().to_string(), src, key, message).into()
}

pub(crate) fn read_bounded(path: &Path, max: u64) -> std::result::Result<Vec<u8>, String> {
    let problem = io_problem(path);
    if !fs::metadata(path).map_err(problem)?.is_file() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    let file = fs::File::open(path).map_err(problem)?;
    read_up_to(path, file, max)
}

fn read_unlinked(path: &Path, max: u64) -> std::result::Result<Vec<u8>, String> {
    let problem = io_problem(path);
    let Some(checked) = unlinked(path).map_err(problem)? else {
        return Err(format!("{}: a symbolic link", path.display()));
    };
    if !checked.is_file() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    let file = fs::File::open(path).map_err(problem)?;
    if !same_file(&checked, &file.metadata().map_err(problem)?) {
        return Err(format!("{}: replaced while read", path.display()));
    }
    read_up_to(path, file, max)
}

pub(crate) fn unlinked(path: &Path) -> std::io::Result<Option<fs::Metadata>> {
    let meta = fs::symlink_metadata(path)?;
    Ok((!meta.file_type().is_symlink()).then_some(meta))
}

#[cfg(unix)]
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    (a.dev(), a.ino()) == (b.dev(), b.ino())
}

#[cfg(not(unix))]
fn same_file(_: &fs::Metadata, _: &fs::Metadata) -> bool {
    true
}

fn io_problem(path: &Path) -> impl Fn(std::io::Error) -> String + Copy + '_ {
    move |e| format!("{}: {}", path.display(), e.kind())
}

fn read_up_to(path: &Path, file: fs::File, max: u64) -> std::result::Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(io_problem(path))?;
    match bytes.len() as u64 > max {
        true => Err(format!("{}: larger than {max} bytes", path.display())),
        false => Ok(bytes),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::{fs, path::PathBuf};

    use super::Serve;

    #[cfg(unix)]
    fn linked(case: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("meada-config-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("outside");
        fs::write(&target, "host secret\n").unwrap();
        let link = dir.join("meada.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        (dir, link, target)
    }

    #[cfg(all(
        unix,
        any(
            feature = "ui",
            feature = "valkey",
            feature = "snapshot",
            feature = "postgres"
        )
    ))]
    #[test]
    fn an_operator_file_through_a_symbolic_link_is_read() {
        let (dir, link, _) = linked("operator");
        assert_eq!(super::read_bounded(&link, 64).unwrap(), b"host secret\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_mirror_file_through_a_symbolic_link_is_refused() {
        let (dir, link, target) = linked("mirror");
        let e = super::read_unlinked(&link, 64).unwrap_err();
        assert_eq!(e, format!("{}: a symbolic link", link.display()));
        let e = super::text(&link, Some(64)).unwrap_err().to_string();
        assert_eq!(e, format!("{}: a symbolic link", link.display()));
        fs::remove_file(&target).unwrap();
        let e = super::text(&link, Some(64)).unwrap_err().to_string();
        assert_eq!(e, format!("{}: a symbolic link", link.display()));
        fs::remove_file(&link).unwrap();
        fs::write(&link, "[serve]\n").unwrap();
        assert_eq!(super::read_unlinked(&link, 8).unwrap(), b"[serve]\n");
        let e = super::read_unlinked(&link, 7).unwrap_err();
        assert_eq!(e, format!("{}: larger than 7 bytes", link.display()));
        fs::write(&target, "").unwrap();
        let meta = |path: &PathBuf| fs::symlink_metadata(path).unwrap();
        assert!(super::same_file(&meta(&link), &meta(&link)));
        assert!(!super::same_file(&meta(&link), &meta(&target)));
        fs::remove_dir_all(&dir).unwrap();
    }

    fn config_of(case: &str, src: &str, max: Option<u64>) -> Result<super::Config, String> {
        let dir =
            std::env::temp_dir().join(format!("meada-base-url-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("meada.toml"), src).unwrap();
        let read = super::read_config_up_to(&dir, max);
        std::fs::remove_dir_all(&dir).unwrap();
        read.map(|c| c.unwrap()).map_err(|e| e.to_string())
    }

    #[test]
    fn a_base_url_declaration_maps_each_variable_to_its_directory() {
        let src = "[[base_url]]\nenv = [\"A_URL\", \"B_URL\"]\ndir = \"svc-b\"\n";
        let config = config_of("maps", src, None).unwrap();
        assert_eq!(config.declared_dir("A_URL"), Some("svc-b"));
        assert_eq!(config.declared_dir("B_URL"), Some("svc-b"));
        assert_eq!(config.declared_dir("C_URL"), None);
    }

    #[test]
    fn a_trailing_slash_is_dropped() {
        let src = "[[base_url]]\nenv = [\"A_URL\"]\ndir = \"svc-b/\"\n";
        let config = config_of("slash", src, None).unwrap();
        assert_eq!(config.declared_dir("A_URL"), Some("svc-b"));
    }

    #[test]
    fn a_bad_directory_is_refused() {
        for (n, dir) in ["", ".", "./svc", "/svc", "svc/../x", "svc\\\\b"]
            .iter()
            .enumerate()
        {
            let src = format!("[serve]\n\n[[base_url]]\nenv = [\"A_URL\"]\ndir = \"{dir}\"\n");
            let e = config_of(&format!("bad{n}"), &src, None).err().unwrap();
            assert!(e.contains("meada.toml:5: `base_url.dir`"), "{dir:?}: {e}");
        }
    }

    #[test]
    fn a_variable_declared_twice_is_refused() {
        let src = "[[base_url]]\nenv = [\"A_URL\"]\ndir = \"a\"\n\n\
                   [[base_url]]\ndir = \"b\"\nenv = [\"B_URL\", \"A_URL\"]\n";
        let e = config_of("twice", src, None).err().unwrap();
        assert!(e.contains("meada.toml:7: `base_url.env`"), "{e}");
        assert!(e.contains("A_URL"), "{e}");
        let src = "base_url = [\n  { env = [\"A_URL\"], dir = \"a\" },\n  \
                   { env = [\"A_URL\"], dir = \"b\" },\n]\n";
        let e = config_of("inline", src, None).err().unwrap();
        assert!(e.contains("meada.toml:3: `base_url.env`"), "{e}");
        let src = "[[base_url]]\nenv = [\"A_URL\",\n  \"A_URL\"]\ndir = \"a\"\n";
        let e = config_of("same", src, None).err().unwrap();
        assert!(e.contains("meada.toml:3: `base_url.env`"), "{e}");
    }

    #[test]
    fn a_missing_directory_is_no_error() {
        let src = "[[base_url]]\nenv = [\"A_URL\"]\ndir = \"gone\"\n";
        let config = config_of("gone", src, None).unwrap();
        assert_eq!(config.declared_dir("A_URL"), Some("gone"));
    }

    #[test]
    fn a_mirror_config_refuses_a_duplicate_too() {
        let src = "[[base_url]]\nenv = [\"A_URL\"]\ndir = \"a\"\n\n\
                   [[base_url]]\nenv = [\"A_URL\"]\ndir = \"b\"\n";
        let e = config_of("mirror", src, Some(super::GENERATION_CONFIG_MAX))
            .err()
            .unwrap();
        assert!(e.contains("meada.toml:6: `base_url.env`"), "{e}");
    }

    #[test]
    fn an_empty_env_is_refused() {
        let src = "[[base_url]]\nenv = []\ndir = \"a\"\n";
        let e = config_of("empty", src, None).err().unwrap();
        assert!(e.contains("meada.toml:2: `base_url.env`"), "{e}");
    }

    #[test]
    fn no_base_url_leaves_serve_undeclared() {
        let src = "[[base_url]]\nenv = [\"A_URL\"]\ndir = \"a\"\n";
        assert!(!config_of("serve", src, None).unwrap().serve.declared);
    }

    #[test]
    fn a_helm_env_declaration_is_read() {
        let src = "[[helm_env]]\nkey = \"lib-chart.envs\"\n";
        let config = config_of("helm", src, None).unwrap();
        let keys: Vec<&str> = config.helm_env.iter().map(|h| h.key.as_str()).collect();
        assert_eq!(keys, ["lib-chart.envs"]);
    }

    #[test]
    fn a_bad_helm_env_key_is_refused() {
        for (n, key) in ["", "a..b", "a.*", ".a", "a."].iter().enumerate() {
            let src = format!("[serve]\n\n[[helm_env]]\nkey = \"{key}\"\n");
            let e = config_of(&format!("helm-bad{n}"), &src, None)
                .err()
                .unwrap();
            assert!(e.contains("meada.toml:4: `helm_env.key`"), "{key:?}: {e}");
        }
    }

    #[test]
    fn a_helm_env_key_declared_twice_is_refused() {
        let src = "[[helm_env]]\nkey = \"a.envs\"\n\n[[helm_env]]\nkey = \"b.envs\"\n\n\
                   [[helm_env]]\nkey = \"a.envs\"\n";
        let e = config_of("helm-twice", src, None).err().unwrap();
        assert!(e.contains("meada.toml:8: `helm_env.key`"), "{e}");
        assert!(e.contains("a.envs"), "{e}");
    }

    #[test]
    fn a_channel_client_is_read() {
        use meada_core::manifest::{Broker, ClientRole};
        let src = "[[channel_client]]\ncall = \"example.com/acme/flow.NewProducer\"\n\
                   role = \"producer\"\nbroker = \"kafka\"\ntopic = \"0.TopicName\"\n\n\
                   [[channel_client]]\ncall = \"svc/kafka/client.go::Client.Consume\"\n\
                   role = \"consumer\"\nbroker = \"kafka\"\ntopic = \"1\"\n";
        let config = config_of("client", src, None).unwrap();
        let [producer, consumer] = &config.channel_clients[..] else {
            panic!("two clients");
        };
        assert_eq!(producer.call, "example.com/acme/flow.NewProducer");
        assert!(producer.role == ClientRole::Producer && producer.broker == Broker::Kafka);
        assert_eq!(producer.topic.arg, 0);
        assert_eq!(producer.topic.field.as_deref(), Some("TopicName"));
        assert_eq!(consumer.call, "svc/kafka/client.go::Client.Consume");
        assert!(consumer.role == ClientRole::Consumer);
        assert_eq!(
            (consumer.topic.arg, consumer.topic.field.as_deref()),
            (1, None)
        );
    }

    fn client(call: &str, topic: &str) -> String {
        format!(
            "[serve]\n\n[[channel_client]]\ncall = \"{call}\"\nrole = \"consumer\"\n\
             broker = \"kafka\"\ntopic = \"{topic}\"\n"
        )
    }

    #[test]
    fn a_bare_type_method_is_refused() {
        for (n, call) in [
            "Client.Consume",
            "",
            "a/b",
            "a/b.",
            "::Client.Consume",
            "x.go::",
        ]
        .iter()
        .enumerate()
        {
            let src = client(call, "1");
            let e = config_of(&format!("bare{n}"), &src, None).err().unwrap();
            assert!(
                e.contains("meada.toml:4: `channel_client.call`"),
                "{call:?}: {e}"
            );
        }
    }

    #[test]
    fn a_bad_topic_position_is_refused() {
        for (n, topic) in ["x", "0.", "-1", "", ".A", "0.A.B", "0.a-b"]
            .iter()
            .enumerate()
        {
            let src = client("example.com/acme/flow.New", topic);
            let e = config_of(&format!("topic{n}"), &src, None).err().unwrap();
            assert!(
                e.contains("meada.toml:7: `channel_client.topic`"),
                "{topic:?}: {e}"
            );
        }
    }

    #[test]
    fn a_channel_client_declared_twice_is_refused() {
        let one = "[[channel_client]]\ncall = \"example.com/acme/flow.New\"\n\
                   role = \"producer\"\nbroker = \"kafka\"\ntopic = \"0\"\n\n";
        let src = format!("{one}{one}");
        let e = config_of("client-twice", &src, None).err().unwrap();
        assert!(e.contains("meada.toml:8: `channel_client.call`"), "{e}");
    }

    #[test]
    fn an_unknown_role_or_broker_is_refused() {
        let src = "[[channel_client]]\ncall = \"example.com/acme/flow.New\"\n\
                   role = \"both\"\nbroker = \"kafka\"\ntopic = \"0\"\n";
        let e = config_of("role", src, None).err().unwrap();
        assert!(e.contains("meada.toml:3: `channel_client.role`"), "{e}");
        let src = "[[channel_client]]\ncall = \"example.com/acme/flow.New\"\n\
                   role = \"producer\"\nbroker = \"rabbitmq\"\ntopic = \"0\"\n";
        let e = config_of("broker", src, None).err().unwrap();
        assert!(e.contains("meada.toml:4: `channel_client.broker`"), "{e}");
    }

    fn serve(prefix: Option<&str>, strip: Option<&str>) -> Serve {
        Serve {
            hosts: Vec::new(),
            documents: Vec::new(),
            prefix: prefix.map(Into::into),
            strip: strip.map(Into::into),
            declared: true,
        }
    }

    #[test]
    fn the_gateway_path_is_the_prefix_then_the_code_path_without_strip() {
        let p = "gateway prefix /shop";
        let s = "gateway strip /v1";
        let both = "gateway prefix /shop, gateway strip /v1";
        for (prefix, strip, code, gateway) in [
            (None, None, "/orders", None),
            (Some("/shop"), None, "/orders", Some(("/shop/orders", p))),
            (None, Some("/v1"), "/v1/orders", Some(("/orders", s))),
            (
                Some("/shop"),
                Some("/v1"),
                "/v1/orders/{}",
                Some(("/shop/orders/{}", both)),
            ),
            (
                Some("/shop"),
                Some("/v1"),
                "/health",
                Some(("/shop/health", p)),
            ),
            (None, Some("/v1"), "/health", None),
            (
                Some("/shop"),
                Some("/v1"),
                "/v10/x",
                Some(("/shop/v10/x", p)),
            ),
            (
                Some("/shop"),
                None,
                "/v1/orders",
                Some(("/shop/v1/orders", p)),
            ),
            (None, Some("/v1"), "/v1", Some(("/", s))),
            (Some("/shop"), Some("/v1"), "/v1", Some(("/shop", both))),
            (Some("/v1"), Some("/v1"), "/v1/orders", None),
        ] {
            let got = serve(prefix, strip).gateway_path(code);
            let want = gateway.map(|(g, r)| (g.to_string(), r.to_string()));
            assert_eq!(got, want, "{prefix:?} {strip:?} {code}");
        }
    }
}
