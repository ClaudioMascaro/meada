//! `meada.toml`: what a repository consumes from its peers (`[[peer]]`), the hosts it serves on
//! (`[serve]`) and the resources it reads or writes (`[[resource]]`, spec #19). Errors name the
//! file, the line and the key.

use std::fs;
use std::ops::Range;
use std::path::Path;

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use toml::de::{DeTable, DeValue};

use crate::msg::Msg;
use crate::{Result, qualify};

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, rename = "peer")]
    pub peers: Vec<Peer>,
    #[serde(default, deserialize_with = "declared")]
    pub serve: Serve,
    #[serde(default, rename = "resource")]
    pub resources: Vec<Resource>,
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
    /// What a gateway adds in front of every path this repository serves.
    #[serde(default, deserialize_with = "gateway_key")]
    pub prefix: Option<String>,
    /// What a gateway removes from the start of every path this repository serves.
    #[serde(default, deserialize_with = "gateway_key")]
    pub strip: Option<String>,
    /// Whether `meada.toml` has a `[serve]` table, even an empty one: only then are the
    /// repository's documents provider sources (spec #21).
    #[serde(skip)]
    pub declared: bool,
}

/// A `[serve]` table that is present: `declared` is set.
fn declared<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Serve, D::Error> {
    let serve = Serve::deserialize(d)?;
    Ok(Serve {
        declared: true,
        ..serve
    })
}

impl Serve {
    /// The path a gateway serves a route at, with the reason: `prefix` followed by the route's
    /// `code_path` without `strip` (only when it starts with `strip`, whole segments). `None`
    /// when no rewrite applies. Code routes use it; it is written for the document routes of
    /// spec #21 (A4) to reuse.
    pub fn gateway_path(&self, code_path: &str) -> Option<(String, String)> {
        let prefix = self.prefix.as_deref().filter(|p| !p.is_empty());
        let strip = self.strip.as_deref().filter(|s| !s.is_empty());
        let stripped = strip.and_then(|s| {
            let rest = code_path.strip_prefix(s)?;
            (rest.is_empty() || rest.starts_with('/')).then_some((s, rest))
        });
        let rest = stripped.map_or(code_path, |(_, rest)| rest);
        let path = format!("{}{rest}", prefix.unwrap_or(""));
        let path = if path.is_empty() { "/".into() } else { path };
        let reasons: Vec<String> = [
            prefix.map(|p| format!("gateway prefix {p}")),
            stripped.map(|(s, _)| format!("gateway strip {s}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!reasons.is_empty() && path != code_path).then(|| (path, reasons.join(", ")))
    }
}

/// A `[serve]` rewrite: it must start with `/`, and is normalized like a path with its
/// trailing `/` dropped; `/` alone is no rewrite.
fn gateway_key<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    let Some(value) = Option::<String>::deserialize(d)? else {
        return Ok(None);
    };
    if !value.starts_with('/') {
        return Err(serde::de::Error::custom(Msg::NoLeadingSlash(&value).text()));
    }
    Ok(Some(
        crate::http::normalize_path(&value)
            .trim_end_matches('/')
            .into(),
    ))
}

/// A shared thing the repository reads or writes, such as a database table or a bucket.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// A lowercase identifier with no meaning to Meada: `postgres`, `s3-bucket`.
    #[serde(deserialize_with = "kind")]
    pub kind: String,
    #[serde(deserialize_with = "not_empty")]
    pub id: String,
    pub role: Role,
    /// The symbols that do the access, as `<path>::<qualified name>` of this repository.
    #[serde(default)]
    pub via: Vec<String>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Read,
    Write,
    ReadWrite,
}

impl Role {
    pub fn writes(self) -> bool {
        self != Role::Read
    }

    pub fn reads(self) -> bool {
        self != Role::Write
    }
}

/// Lowercase ASCII letters, digits and hyphens, starting with a letter.
fn kind<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    let kind = String::deserialize(d)?;
    let mut chars = kind.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    match valid {
        true => Ok(kind),
        false => Err(D::Error::custom(Msg::NotLowercase(&kind).text())),
    }
}

fn not_empty<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    let value = String::deserialize(d)?;
    match value.is_empty() {
        true => Err(D::Error::custom(Msg::Empty.text())),
        false => Ok(value),
    }
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
const LATER: &[&str] = &["serve.documents"];

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

#[cfg(test)]
mod tests {
    use super::Serve;

    fn serve(prefix: Option<&str>, strip: Option<&str>) -> Serve {
        Serve {
            hosts: Vec::new(),
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
            // Nothing declared: no gateway path.
            (None, None, "/orders", None),
            (Some("/shop"), None, "/orders", Some(("/shop/orders", p))),
            (None, Some("/v1"), "/v1/orders", Some(("/orders", s))),
            (
                Some("/shop"),
                Some("/v1"),
                "/v1/orders/{}",
                Some(("/shop/orders/{}", both)),
            ),
            // A code path that does not start with `strip` keeps all of it under `prefix`...
            (
                Some("/shop"),
                Some("/v1"),
                "/health",
                Some(("/shop/health", p)),
            ),
            // ... and has no gateway path without a prefix.
            (None, Some("/v1"), "/health", None),
            // `strip` matches whole segments; a version goes only when it is written.
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
            // Stripping the whole path leaves the root.
            (None, Some("/v1"), "/v1", Some(("/", s))),
            (Some("/shop"), Some("/v1"), "/v1", Some(("/shop", both))),
            // A rewrite that changes nothing is no gateway path.
            (Some("/v1"), Some("/v1"), "/v1/orders", None),
        ] {
            let got = serve(prefix, strip).gateway_path(code);
            let want = gateway.map(|(g, r)| (g.to_string(), r.to_string()));
            assert_eq!(got, want, "{prefix:?} {strip:?} {code}");
        }
    }
}
