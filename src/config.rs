//! `meada.toml`, the v0.1 subset: what a repository consumes from its peers (`[[peer]]`) and
//! the hosts it serves on (`[serve]`). Errors name the file, the line and the key.

use std::fs;
use std::ops::Range;
use std::path::Path;

use serde::Deserialize;
use toml::de::{DeTable, DeValue};

use crate::Result;

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

/// Keys a later release adds, with the release that adds them.
const LATER: &[(&str, &str)] = &[
    ("resource", "track A adds it (resources and gateway)"),
    ("serve.prefix", "track A adds it (resources and gateway)"),
    ("serve.strip", "track A adds it (resources and gateway)"),
];

/// `<root>/meada.toml`, or an empty configuration when the file does not exist.
pub fn load(root: &Path) -> Result<Config> {
    let file = root.join("meada.toml");
    let src = match fs::read_to_string(&file) {
        Ok(src) => src,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(format!("{}: {e}", file.display()).into()),
    };
    toml::from_str(&src).map_err(|e| {
        let at = e.span().map_or(0, |s| s.start);
        let line = src[..at.min(src.len())].matches('\n').count() + 1;
        let what = describe(&src, at, e.message());
        format!("{}:{line}: {what}", file.display()).into()
    })
}

/// The error message with the dotted key it is about.
fn describe(src: &str, at: usize, message: &str) -> String {
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
            format!("`{}` is missing", join(parent.unwrap_or(""), field))
        }
        (Some(_), Some(key)) => match LATER.iter().find(|(later, _)| *later == key) {
            Some((_, release)) => format!("`{key}` is not in v0.1; {release}"),
            None => format!("`{key}` is not a meada.toml key"),
        },
        (_, Some(key)) => format!("`{key}`: {message}"),
        (_, None) => message.to_string(),
    }
}

/// Every key of `table` with its dotted name, key span and value span; a table's keys follow
/// the table itself.
fn flatten(table: &DeTable, prefix: &str, out: &mut Vec<(String, Range<usize>, Range<usize>)>) {
    for (key, value) in table.iter() {
        let name = join(prefix, key.get_ref());
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

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}
