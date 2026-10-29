//! A workspace manifest's `[[repository]]` entries (ADR 0011) and every check their text
//! decides: the shapes of `serve`, `resource` and `peer` (those of `meada.toml`, the peer without
//! `path`), unknown keys, and the identities. Errors name the file, the line and the dotted key.
//! The checks that need a mirror, and the module sections, are the binary's.

use std::collections::HashSet;
use std::ops::Range;

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use toml::de::{DeTable, DeValue};

use crate::msg::Msg;
use crate::{parent, qualify};

#[derive(Deserialize, Default, Clone)]
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
    /// Documents (spec #21) besides those found by file name, repository-relative. One that
    /// `.gitignore` ignores is read all the same.
    #[serde(default, deserialize_with = "repo_paths")]
    pub documents: Vec<String>,
    /// Whether `meada.toml` has a `[serve]` table, even an empty one: only then are the
    /// repository's documents provider sources (spec #21).
    #[serde(skip)]
    pub declared: bool,
}

/// A `[serve]` table that is present: `declared` is set.
pub fn declared<'de, D: Deserializer<'de>>(d: D) -> Result<Serve, D::Error> {
    let serve = Serve::deserialize(d)?;
    Ok(Serve {
        declared: true,
        ..serve
    })
}

impl Serve {
    /// The path a gateway serves a route at, with the reason (see `gateway_path`), for the
    /// repository's `[serve]` prefix and strip.
    pub fn gateway_path(&self, code_path: &str) -> Option<(String, String)> {
        gateway_path(self.prefix.as_deref(), self.strip.as_deref(), code_path)
    }
}

/// The path a gateway serves a route at, with the reason: `prefix` followed by the route's
/// `code_path` without `strip` (only when it starts with `strip`, whole segments). `None`
/// when no rewrite applies. Code routes use the `[serve]` pair; the routes of a document
/// (spec #21) use the path of a `servers` URL as the prefix and the `[serve]` strip.
pub fn gateway_path(
    prefix: Option<&str>,
    strip: Option<&str>,
    code_path: &str,
) -> Option<(String, String)> {
    let prefix = prefix.filter(|p| !p.is_empty());
    let strip = strip.filter(|s| !s.is_empty());
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

/// Repository-relative paths: none may be absolute or climb out with `..`. `.` segments and
/// doubled `/` are dropped.
fn repo_paths<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let mut paths = Vec::new();
    for path in Vec::<String>::deserialize(d)? {
        let segments: Vec<&str> = path.split('/').collect();
        if path.starts_with('/') || segments.contains(&"..") {
            return Err(D::Error::custom(Msg::NotInRepository(&path).text()));
        }
        paths.push(
            segments
                .into_iter()
                .filter(|s| !matches!(*s, "" | "."))
                .collect::<Vec<_>>()
                .join("/"),
        );
    }
    Ok(paths)
}

/// A `[serve]` rewrite: it must start with `/`, and is normalized like a path with its
/// trailing `/` dropped; `/` alone is no rewrite.
fn gateway_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let Some(value) = Option::<String>::deserialize(d)? else {
        return Ok(None);
    };
    if !value.starts_with('/') {
        return Err(D::Error::custom(Msg::NoLeadingSlash(&value).text()));
    }
    Ok(Some(
        crate::http::normalize_path(&value)
            .trim_end_matches('/')
            .into(),
    ))
}

/// A shared thing the repository reads or writes, such as a database table or a bucket.
#[derive(Deserialize, Clone)]
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
fn kind<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let kind = String::deserialize(d)?;
    let mut chars = kind.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    match valid {
        true => Ok(kind),
        false => Err(D::Error::custom(Msg::NotLowercase(&kind).text())),
    }
}

fn not_empty<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    match value.is_empty() {
        true => Err(D::Error::custom(Msg::Empty.text())),
        false => Ok(value),
    }
}

/// The most characters an entry's `owner` may hold.
// ponytail: an owner names one OIDC group, so 256 characters is ample; a longer name is refused
// where it is read (ADR 0015).
pub const OWNER_MAX: usize = 256;

/// What an `owner` may not be (ADR 0015): empty, over `OWNER_MAX` characters, holding a control
/// character (Unicode category Cc) or with whitespace at either end.
fn owner_problem(name: &str) -> Option<Msg<'static>> {
    let padded = name.chars().next().is_some_and(char::is_whitespace)
        || name.chars().next_back().is_some_and(char::is_whitespace);
    if name.is_empty() {
        Some(Msg::Empty)
    } else if name.chars().count() > OWNER_MAX {
        Some(Msg::OwnerTooLong)
    } else if name.chars().any(char::is_control) {
        Some(Msg::OwnerControl)
    } else if padded {
        Some(Msg::OwnerPadded)
    } else {
        None
    }
}

/// One string that `owner_problem` accepts; an array or a number is not an owner.
fn owner<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let toml::Value::String(name) = toml::Value::deserialize(d)? else {
        return Err(D::Error::custom(Msg::OwnerNotString.text()));
    };
    match owner_problem(&name) {
        Some(problem) => Err(D::Error::custom(problem.text())),
        None => Ok(Some(name)),
    }
}

/// The most characters a GitHub login holds.
pub const ADDED_BY_MAX: usize = 39;

/// What an `added_by` may not be (#330): empty, over `ADDED_BY_MAX` characters or holding
/// anything but lowercase ASCII letters, digits, `-` and `_`, so a login has one canonical text.
pub fn added_by_problem(login: &str) -> Option<Msg<'static>> {
    if login.is_empty() {
        Some(Msg::Empty)
    } else if login.chars().count() > ADDED_BY_MAX {
        Some(Msg::AddedByTooLong)
    } else if !login
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
    {
        Some(Msg::AddedByChars)
    } else {
        None
    }
}

/// One string that `added_by_problem` accepts; an array or a number is not a login.
fn added_by<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let toml::Value::String(login) = toml::Value::deserialize(d)? else {
        return Err(D::Error::custom(Msg::AddedByNotString.text()));
    };
    match added_by_problem(&login) {
        Some(problem) => Err(D::Error::custom(problem.text())),
        None => Ok(Some(login)),
    }
}

/// A `[[repository]]`: `serve` and `resource` take the shapes of `meada.toml`'s, `peer` that
/// of its `[[peer]]` without `path` (the manifest locates every repository).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryFile {
    pub repo: String,
    pub path: String,
    /// The OIDC group that answers for the repository (ADR 0015): entry metadata of a workspace
    /// version, not a block; nothing here reads it to decide anything.
    #[serde(default, deserialize_with = "owner")]
    pub owner: Option<String>,
    /// The GitHub login that added the entry (#330): metadata like `owner`, attribution and no
    /// grant; nothing reads it to decide anything yet.
    #[serde(default, deserialize_with = "added_by")]
    pub added_by: Option<String>,
    #[serde(default, deserialize_with = "declared")]
    pub serve: Serve,
    #[serde(default, rename = "resource")]
    pub resources: Vec<Resource>,
    #[serde(default, rename = "peer")]
    pub peers: Vec<EntryPeer>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPeer {
    pub repo: String,
    #[serde(default)]
    pub base_url_env: Vec<String>,
}

impl EntryFile {
    /// Its blocks, as `status` names those a mirror's own `meada.toml` overrides:
    /// `[repository.serve]`, `[[repository.resource]] <kind> <id>`, `[[repository.peer]] <repo>`.
    pub fn blocks(&self) -> Vec<String> {
        let serve = self
            .serve
            .declared
            .then(|| "[repository.serve]".to_string());
        let resources =
            (self.resources.iter()).map(|r| format!("[[repository.resource]] {} {}", r.kind, r.id));
        let peers = (self.peers.iter()).map(|p| format!("[[repository.peer]] {}", p.repo));
        serve.into_iter().chain(resources).chain(peers).collect()
    }
}

/// Why `repo` cannot be the identity of a served repository, checked before any path is built
/// from it: a segment that would leave the directory it is joined to (empty, `.` or `..`), or a
/// `local/` identity. A manifest entry and a generation document (`meada snapshot pull`) share
/// it; each also refuses an identity listed twice.
pub fn identity_problem(repo: &str) -> Option<String> {
    if repo.split('/').any(|s| matches!(s, "" | "." | "..")) {
        return Some(Msg::UnsafeIdentity.text());
    }
    if repo.starts_with("local/") {
        return Some(Msg::LocalIdentity.text());
    }
    None
}

/// Each entry's problem that its identity alone decides, in manifest order: `identity_problem`,
/// else an identity an earlier entry lists already.
pub fn entry_problems<'a>(repos: impl IntoIterator<Item = &'a str>) -> Vec<Option<String>> {
    let mut seen = HashSet::new();
    (repos.into_iter())
        .map(|repo| {
            let twice = !seen.insert(repo);
            identity_problem(repo).or_else(|| twice.then(|| Msg::ListedTwice.text()))
        })
        .collect()
}

/// The text of a workspace: its `[[repository]]` entries and nothing else.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entries {
    #[serde(default, rename = "repository")]
    repositories: Vec<EntryFile>,
}

/// The `[[repository]]` entries of a whole manifest's text, every other key left unread: what
/// `meada workspace import` takes from a file.
#[derive(Deserialize)]
struct Imported {
    #[serde(default, rename = "repository")]
    repositories: Vec<EntryFile>,
}

/// The entries of the workspace text `src`, shown as `file`, or every problem its text decides
/// as the CLI prints them: the parse error (`<file>:<line>: <what>`), or each entry's
/// `<repo>: <problem>`.
pub fn validate(file: &str, src: &str) -> Result<Vec<EntryFile>, Vec<String>> {
    let parsed: Entries = parse(file, src, "meada.workspace.toml").map_err(|e| vec![e])?;
    checked(parsed.repositories)
}

/// `validate` over the `[[repository]]` entries of the manifest text `src` alone: its other
/// top-level keys and sections are not read (`meada workspace import`).
pub fn validate_entries(file: &str, src: &str) -> Result<Vec<EntryFile>, Vec<String>> {
    let parsed: Imported = parse(file, src, "meada.workspace.toml").map_err(|e| vec![e])?;
    checked(parsed.repositories)
}

/// The entry-only validation of `meada workspace import`, including the file prefix on
/// identity errors. Other top-level keys are ignored; the CLI separately reports its notes.
pub fn validate_import(file: &str, src: &str) -> Result<Vec<EntryFile>, Vec<String>> {
    validate_entries(file, src).map_err(|problems| {
        problems
            .into_iter()
            .map(|problem| {
                if problem.starts_with(file) {
                    problem
                } else {
                    format!("{file}: {problem}")
                }
            })
            .collect()
    })
}

/// `entries`, or each one's `<repo>: <problem>` that its identity decides.
fn checked(entries: Vec<EntryFile>) -> Result<Vec<EntryFile>, Vec<String>> {
    let problems = entry_problems(entries.iter().map(|e| e.repo.as_str()));
    let problems: Vec<String> = (entries.iter().zip(problems))
        .filter_map(|(e, problem)| Some(format!("{}: {}", e.repo, problem?)))
        .collect();
    match problems.is_empty() {
        true => Ok(entries),
        false => Err(problems),
    }
}

/// `src`, the content of `file`, parsed as `T`; an error names the file, the line and the key,
/// `name` being the kind of file the key belongs to (`meada.toml`, `meada.workspace.toml`).
pub fn parse<T: DeserializeOwned>(file: &str, src: &str, name: &str) -> Result<T, String> {
    toml::from_str(src).map_err(|e| {
        let at = e.span().map_or(0, |s| s.start);
        let line = line_of(src, at);
        let what = describe(src, at, e.message(), name);
        format!("{file}:{line}: {what}")
    })
}

/// The 1-based line of the byte offset `at` in `src`.
pub fn line_of(src: &str, at: usize) -> usize {
    src[..at.min(src.len())].matches('\n').count() + 1
}

/// An error about the dotted `key` of `src`, the content of `file`, as `parse` words one: the
/// file, the line of the key (of its table when the key is absent, its default being the
/// problem; of the file when neither is there), the key and the message.
pub fn error_at(file: &str, src: &str, key: &str, message: &str) -> String {
    let mut keys = Vec::new();
    if let Ok(table) = DeTable::parse(src) {
        flatten(table.get_ref(), "", &mut keys);
    }
    let start = |key: &str| {
        let found = keys.iter().find(|(name, _, _)| name == key);
        found.map(|(_, span, _)| span.start)
    };
    let at = start(key).or_else(|| start(parent(key))).unwrap_or(0);
    format!("{file}:{}: `{key}`: {message}", line_of(src, at))
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
