//! `meada.toml`: what a repository consumes from its peers (`[[peer]]`), the hosts it serves on
//! (`[serve]`) and the resources it reads or writes (`[[resource]]`, spec #19). Errors name the
//! file, the line and the key. A workspace manifest entry reuses these shapes for a mirror with
//! no `meada.toml` (`workspace::Manifest`, ADR 0011). The shapes and the text's checks are
//! `meada_core::manifest`'s; this module reads the files.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

pub use meada_core::manifest::{Resource, Serve, declared, gateway_path, line_of};

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
    /// The peers whose base URL this repository reads from the variable `var`.
    pub fn peers_reading<'a>(&'a self, var: &'a str) -> impl Iterator<Item = &'a str> {
        let reads = move |p: &&Peer| p.base_url_env.iter().any(|v| v == var);
        self.peers.iter().filter(reads).map(|p| p.repo.as_str())
    }
}

/// `<root>/meada.toml`, or an empty configuration when the file does not exist.
pub fn load(root: &Path) -> Result<Config> {
    Ok(load_own(root)?.unwrap_or_default())
}

/// `<root>/meada.toml`, or `None` when the file does not exist.
pub fn load_own(root: &Path) -> Result<Option<Config>> {
    read_own(root, "meada.toml")
}

/// `<root>/<name>` parsed as `T`, or `None` when the file does not exist. A parse error names
/// the file, the line and the key; `meada.decisions.toml` is read the same way.
fn read_own<T: DeserializeOwned>(root: &Path, name: &str) -> Result<Option<T>> {
    read_own_up_to(root, name, None)
}

/// The most a mirror's `meada.toml` or `meada.decisions.toml` holds in a pulled generation
/// (ADR 0012), whose files are hostile; a larger one fails the generation, naming the file.
#[cfg(feature = "snapshot")]
// ponytail: a flat 1 MiB, as `lang::MAX_BYTES`; real ones are a few kilobytes.
pub(crate) const GENERATION_CONFIG_MAX: u64 = 1 << 20;

/// `read_own`, reading at most `max` bytes (`read_bounded`) when it is set.
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

/// The text of `file`, `None` when it does not exist; with `max`, read as `read_bounded` reads,
/// a regular file of at most `max` bytes.
pub(crate) fn text(file: &Path, max: Option<u64>) -> Result<Option<String>> {
    let missing = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    let Some(max) = max else {
        return match fs::read_to_string(file) {
            Ok(src) => Ok(Some(src)),
            Err(e) if missing(&e) => Ok(None),
            Err(e) => Err(format!("{}: {e}", file.display()).into()),
        };
    };
    if fs::metadata(file).is_err_and(|e| missing(&e)) {
        return Ok(None);
    }
    let bytes = read_bounded(file, max)?;
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

/// An error about the dotted `key` of `src`, the content of `file`, as `parse` words one
/// (`meada_core::manifest::error_at`).
pub(crate) fn error_at(
    file: &Path,
    src: &str,
    key: &str,
    message: &str,
) -> Box<dyn std::error::Error> {
    meada_core::manifest::error_at(&file.display().to_string(), src, key, message).into()
}

/// The content of the regular file `path` a cloud module's section names (a secret, a CA), at
/// most `max` bytes. What is not a regular file is refused before it is opened: a FIFO would
/// block the open for ever.
pub(crate) fn read_bounded(path: &Path, max: u64) -> std::result::Result<Vec<u8>, String> {
    use std::io::Read;
    let problem = |e: std::io::Error| format!("{}: {}", path.display(), e.kind());
    if !fs::metadata(path).map_err(problem)?.is_file() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    let mut bytes = Vec::new();
    let file = fs::File::open(path).map_err(problem)?;
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(problem)?;
    match bytes.len() as u64 > max {
        true => Err(format!("{}: larger than {max} bytes", path.display())),
        false => Ok(bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::Serve;

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
