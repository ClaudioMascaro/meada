//! `meada.toml`, the v0.1 subset: what a repository consumes from its peers (`[[peer]]`) and
//! the hosts it serves on (`[serve]`). Errors name the file, the line and the key.

use std::fs;
use std::ops::Range;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use toml::de::{DeTable, DeValue};

use crate::msg::Msg;
use crate::{Result, qualify};

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, rename = "peer")]
    pub peers: Vec<Peer>,
    #[serde(default)]
    pub serve: Serve,
}

#[derive(Deserialize)]
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

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Serve {
    #[serde(default)]
    pub hosts: Vec<String>,
}

impl Config {
    /// Whether this repository serves on `host`, case-insensitively.
    pub fn serves(&self, host: &str) -> bool {
        self.serve
            .hosts
            .iter()
            .any(|h| h.eq_ignore_ascii_case(host))
    }

    /// The peers whose base URL this repository reads from the variable `var`.
    pub fn peers_reading<'a>(&'a self, var: &'a str) -> impl Iterator<Item = &'a str> {
        let reads = move |p: &&Peer| p.base_url_env.iter().any(|v| v == var);
        self.peers.iter().filter(reads).map(|p| p.repo.as_str())
    }
}

/// Keys that track A adds after v0.1.
const LATER: &[&str] = &["resource", "serve.prefix", "serve.strip"];

/// `<root>/meada.toml`, or an empty configuration when the file does not exist.
pub fn load(root: &Path) -> Result<Config> {
    read(root, "meada.toml")
}

/// `<root>/<name>` parsed as `T`, or `T::default()` when the file does not exist. A parse error
/// names the file, the line and the key; `meada.decisions.toml` is read the same way.
pub(crate) fn read<T: DeserializeOwned + Default>(root: &Path, name: &str) -> Result<T> {
    let file = root.join(name);
    let src = match fs::read_to_string(&file) {
        Ok(src) => src,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(format!("{}: {e}", file.display()).into()),
    };
    toml::from_str(&src).map_err(|e| {
        let at = e.span().map_or(0, |s| s.start);
        let line = src[..at.min(src.len())].matches('\n').count() + 1;
        let what = describe(&src, at, e.message(), name);
        format!("{}:{line}: {what}", file.display()).into()
    })
}

/// The error message with the dotted key it is about.
fn describe(src: &str, at: usize, message: &str, file: &str) -> String {
    // A syntax error leaves no table to find the key in.
    let Ok(table) = DeTable::parse(src) else {
        return message.to_string();
    };
    let mut keys = Vec::new();
    flatten(table.get_ref(), "", &mut keys);
    // Parents come before their children, so the last key around `at` is the innermost.
    let around = keys
        .iter()
        .rfind(|(_, key, value)| key.contains(&at) || value.contains(&at))
        .map(|(name, _, _)| name.as_str());
    let field = message
        .split('`')
        .nth(1)
        .filter(|_| message.starts_with("unknown field") || message.starts_with("missing field"));
    match (field, around) {
        (Some(field), parent) if message.starts_with("missing field") => {
            Msg::KeyMissing(&qualify(parent.unwrap_or(""), field)).text()
        }
        (Some(_), Some(key)) if file == "meada.toml" && LATER.contains(&key) => Msg::KeyLater {
            key,
            release: &Msg::TrackA.text(),
        }
        .text(),
        (Some(_), Some(key)) => Msg::KeyUnknown { key, file }.text(),
        (_, Some(key)) => format!("`{key}`: {message}"),
        (_, None) => message.to_string(),
    }
}

/// Every key of `table` with its dotted name, key span and value span; a table's keys follow
/// the table itself.
fn flatten(table: &DeTable, prefix: &str, out: &mut Vec<(String, Range<usize>, Range<usize>)>) {
    for (key, value) in table.iter() {
        let name = qualify(prefix, key.get_ref());
        out.push((name.clone(), key.span(), value.span()));
        match value.get_ref() {
            DeValue::Table(inner) => flatten(inner, &name, out),
            DeValue::Array(items) => {
                for item in items.iter() {
                    if let DeValue::Table(inner) = item.get_ref() {
                        flatten(inner, &name, out);
                    }
                }
            }
            _ => {}
        }
    }
}
