//! Config values (ADR 0025, #639): every scalar of the YAML and JSON files the walk visits, and
//! of the CI configuration under the hidden paths `HIDDEN_CI`, kept with its file, key path,
//! value, type and line, which `search` finds with the kind `value`. Read through the event
//! reader (`document::events`); the rows live in `config_values` beside the stamped
//! `config_files`, which keeps each file's `Outcome` for `meada status`. Never exported.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write;
use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;
use crate::contract::Hit;
use crate::document::events::{self, Step, Syntax};
use crate::lang;
use crate::msg::Msg;

mod hcl;
pub(crate) mod sites;

pub(crate) use hcl::nested_past;

/// The lockfiles a YAML or JSON extension reaches, skipped by file name (ADR 0025).
const LOCKFILES: [&str; 4] = [
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "packages.lock.json",
];

/// A YAML/JSON dependency lockfile is not application configuration.
pub(crate) fn lockfile(path: &str) -> bool {
    LOCKFILES.contains(&path.rsplit('/').next().unwrap_or(path))
}

/// The hidden entries of the repository root read although hidden (ADR 0025): the CI
/// directories, read whole, and the CI files.
pub(crate) const HIDDEN_CI: [&str; 6] = [
    ".github",
    ".gitlab-ci.yml",
    ".gitlab",
    ".circleci",
    ".buildkite",
    ".drone.yml",
];

/// The bytes of a value kept; a longer one is stored cut at a character boundary, marked.
// ponytail: 1 KiB keeps a URL, an image name or an expression whole; past it a value is prose.
pub(crate) const VALUE_MAX: usize = 1024;

/// The documents one file may hold; a stream of more is unreadable.
// ponytail: a manifest bundle holds tens of documents; past a thousand it is generated output.
pub(crate) const DOCUMENTS_MAX: usize = 1000;

/// The bytes the rows of one file may store (key paths and values), as a factor of its size,
/// plus `STORED_SLACK`: a file of long keys nested deep, each repeated in the key path of every
/// scalar below it, cannot store a tree many times its size. Past it the file is unreadable.
// ponytail: an indented file stores about twice its size; 4 is the hostile probes' bound, and
// keeps a minified file of tiny scalars (a long array of numbers) out.
const STORED_EXPANSION: usize = 4;
const STORED_SLACK: usize = 1 << 12;

/// What reading one config file came to, stored as `config_files.summary`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Outcome {
    /// Read: how many values it holds, and how many of them are `redacted`. A `.tfvars` file also
    /// keeps the sensitive variables it was read against (`Sensitive`), since a variable
    /// another file marks `sensitive` redacts the values this one gives it.
    Read {
        values: usize,
        redacted: usize,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sensitive: Vec<String>,
    },
    /// A lockfile, skipped by name.
    Lockfile,
    /// Skipped: it does not parse, or passes a ceiling, at `line`.
    Unreadable { line: usize, reason: String },
}

/// One config value as stored.
#[derive(Debug, PartialEq)]
pub(crate) struct Row {
    pub key: String,
    /// `None` when a format marks it secret: only its key path, type and line are kept.
    pub value: Option<String>,
    /// `string`, `number` or `boolean`.
    pub kind: &'static str,
    pub line: usize,
    /// Whether `value` was cut at `VALUE_MAX`.
    pub cut: bool,
}

/// Whether the file at `path` (relative, `/`-separated) holds config values: a `*.yaml`,
/// `*.yml` or `*.json` file, or a Terraform or OpenTofu one (`*.tf`, `*.tofu`, `*.tfvars`),
/// lockfiles included (they are counted as skipped).
pub(crate) fn is_config(path: &str) -> bool {
    [".yaml", ".yml", ".json", ".tf", ".tofu", ".tfvars"]
        .iter()
        .any(|e| path.ends_with(e))
}

/// Whether the file at `path` gives values to variables: `*.tfvars`, `*.auto.tfvars` or
/// `*.tfvars.json`.
pub(crate) fn is_tfvars(path: &str) -> bool {
    path.ends_with(".tfvars") || path.ends_with(".tfvars.json")
}

/// `found` without the Terraform files a OpenTofu one shadows: a `.tofu` file hides the `.tf`
/// file of the same name in its directory, and a `.tofu.json` the `.tf.json` (ADR 0025).
pub(crate) fn unshadowed(found: Vec<(String, PathBuf)>) -> Vec<(String, PathBuf)> {
    let paths: HashSet<String> = found.iter().map(|(p, _)| p.clone()).collect();
    let shadowed = |path: &str| {
        [(".tf", ".tofu"), (".tf.json", ".tofu.json")]
            .iter()
            .any(|(tf, tofu)| {
                (path.strip_suffix(tf)).is_some_and(|stem| paths.contains(&format!("{stem}{tofu}")))
            })
    };
    found.into_iter().filter(|(p, _)| !shadowed(p)).collect()
}

/// The parent directory of `path`, `""` for the root.
pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// What a `.tfvars` file's values are redacted for in place of a variable's name: every value it
/// gives. Part of `Sensitive::of` when a Terraform file that may declare variables is unreadable.
pub(crate) const ANY_VARIABLE: &str = "*";

/// The sensitive variables a `.tfvars` file is read against: the names declared `sensitive = true`
/// by any Terraform file found (a var-file can be given to a module in any directory, and
/// redacting too much is the safe direction), and `ANY_VARIABLE` when a Terraform file in its
/// directory or above it is unreadable, since the variables it declares are unknown. Computed
/// once per refresh from the rows of the files found, after every file but the `.tfvars` ones is
/// refreshed.
pub(crate) struct Sensitive<'a> {
    db: &'a Connection,
    found: HashSet<&'a str>,
    /// The names, and the directories holding an unreadable Terraform file.
    loaded: RefCell<Option<(BTreeSet<String>, Vec<String>)>>,
}

impl<'a> Sensitive<'a> {
    pub(crate) fn new(db: &'a Connection, found: &'a [(String, PathBuf)]) -> Self {
        Sensitive {
            db,
            found: found.iter().map(|(p, _)| p.as_str()).collect(),
            loaded: RefCell::new(None),
        }
    }

    /// The sensitive variables the file at `path` is read against; none unless it is a
    /// `.tfvars` file.
    pub(crate) fn of(&self, path: &str) -> Result<BTreeSet<String>> {
        if !is_tfvars(path) {
            return Ok(BTreeSet::new());
        }
        let mut loaded = self.loaded.borrow_mut();
        if loaded.is_none() {
            *loaded = Some(self.load()?);
        }
        let (names, unreadable) = loaded.as_ref().expect("loaded above");
        let mut names = names.clone();
        let dir = parent(path);
        let below = |d: &String| {
            d.is_empty()
                || dir == d
                || dir
                    .strip_prefix(d.as_str())
                    .is_some_and(|r| r.starts_with('/'))
        };
        if unreadable.iter().any(below) {
            names.insert(ANY_VARIABLE.to_string());
        }
        Ok(names)
    }

    /// Whether the stored rows of the `.tfvars` file at `path` were read against other
    /// sensitive variables than those now: they are read again though the file did not change.
    pub(crate) fn stale(&self, path: &str) -> Result<bool> {
        if !is_tfvars(path) {
            return Ok(false);
        }
        let summary: Option<String> = self
            .db
            .query_row(
                "SELECT summary FROM config_files WHERE path = ?1",
                params![path],
                |r| r.get(0),
            )
            .optional()?;
        let Some(Outcome::Read { sensitive, .. }) =
            summary.and_then(|s| serde_json::from_str(&s).ok())
        else {
            return Ok(false);
        };
        Ok(self.of(path)?.into_iter().collect::<Vec<_>>() != sensitive)
    }

    fn load(&self) -> Result<(BTreeSet<String>, Vec<String>)> {
        let mut statement = self.db.prepare(
            "SELECT path, key FROM config_values \
             WHERE type = 'boolean' AND value = 'true' AND key LIKE 'variable%.sensitive'",
        )?;
        let mut names = BTreeSet::new();
        for row in
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (path, key) = row?;
            if let (true, Some(name)) = (self.found.contains(path.as_str()), variable(&key)) {
                names.insert(name);
            }
        }
        let mut statement = self.db.prepare("SELECT path, summary FROM config_files")?;
        let mut unreadable = Vec::new();
        for row in
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (path, summary) = row?;
            let terraform = [".tf", ".tofu", ".tf.json", ".tofu.json"]
                .iter()
                .any(|e| path.ends_with(e));
            if terraform
                && self.found.contains(path.as_str())
                && matches!(
                    serde_json::from_str(&summary),
                    Ok(Outcome::Unreadable { .. })
                )
            {
                unreadable.push(parent(&path).to_string());
            }
        }
        Ok((names, unreadable))
    }
}

/// The name of the variable whose `sensitive` attribute the key path `key` is
/// (`variable.db_password.sensitive`, `variable["db-password"].sensitive`).
fn variable(key: &str) -> Option<String> {
    let rest = key.strip_prefix("variable")?.strip_suffix(".sensitive")?;
    if let Some(name) = rest.strip_prefix('.') {
        return plain(name).then(|| name.to_string());
    }
    let quoted = rest.strip_prefix('[')?.strip_suffix(']')?;
    serde_json::from_str(quoted).ok()
}

/// Whether a hidden entry the walk meets at the root, named `name`, is read anyway.
pub(crate) fn is_hidden_ci(name: &str) -> bool {
    HIDDEN_CI.contains(&name)
}

/// What a scalar row is before its document is known: its document, the steps that tell
/// whether a Secret holds it, and the row.
struct Pending {
    document: usize,
    secret: SecretPlace,
    row: Row,
}

/// Where a scalar sits relative to the places a format keeps secret values.
#[derive(Clone, PartialEq)]
enum SecretPlace {
    No,
    /// Under the root's `data` or `stringData` of a Kubernetes `Secret`.
    Root,
    /// Under `items[n].data` or `items[n].stringData` of a list.
    Item(usize),
    /// Under `variable.<name>.default` of a Terraform JSON file: secret when the variable says
    /// `"sensitive": true`.
    Variable(String),
    /// Under the top-level key `<name>` of a `.tfvars.json` file: secret when the variable is
    /// sensitive.
    Tfvars(String),
    /// Under `Parameters.<name>.Default`: secret when the file is a CloudFormation template and
    /// the parameter says `NoEcho`.
    Parameter(String),
    /// An SSM parameter value in Terraform/OpenTofu JSON; its type controls redaction.
    SsmParameter(String),
}

impl SecretPlace {
    fn of(path: &[Step], file: &str) -> SecretPlace {
        let held =
            |s: Option<&Step>| matches!(s, Some(Step::Key(k)) if k == "data" || k == "stringData");
        match path {
            [Step::Key(name), ..] if file.ends_with(".tfvars.json") => {
                SecretPlace::Tfvars(name.clone())
            }
            [Step::Key(v), Step::Key(name), Step::Key(d), ..]
                if v == "variable" && d == "default" && terraform_json(file) =>
            {
                SecretPlace::Variable(name.clone())
            }
            [
                Step::Key(r),
                Step::Key(t),
                Step::Key(name),
                Step::Key(v),
                ..,
            ] if r == "resource"
                && t == "aws_ssm_parameter"
                && v == "value"
                && terraform_json(file) =>
            {
                SecretPlace::SsmParameter(name.clone())
            }
            [Step::Key(p), Step::Key(name), Step::Key(d), ..]
                if p == "Parameters" && d == "Default" =>
            {
                SecretPlace::Parameter(name.clone())
            }
            [first, ..] if held(Some(first)) => SecretPlace::Root,
            [Step::Key(items), Step::Index(n), rest @ ..]
                if items == "items" && held(rest.first()) =>
            {
                SecretPlace::Item(*n)
            }
            _ => SecretPlace::No,
        }
    }
}

/// Only an explicitly public SSM type permits storing its value. An unresolved type is
/// conservatively secret in both HCL and JSON; absence is handled by their attribute readers.
fn ssm_value_is_secret(kind: Option<&str>) -> bool {
    !matches!(kind, Some("String" | "StringList"))
}

/// Whether `file` is a Terraform or OpenTofu JSON file (`*.tf.json`, `*.tofu.json`).
fn terraform_json(file: &str) -> bool {
    file.ends_with(".tf.json") || file.ends_with(".tofu.json")
}

/// The type of a string of a Terraform JSON file, where every string is a template: `string`
/// when it holds no interpolation or directive (a doubled `$` or `%` escapes one), `expression`
/// when it is one interpolation and nothing else, else `template`.
fn json_kind(s: &str) -> &'static str {
    let bare = s.replace("$${", "").replace("%%{", "");
    if !bare.contains("${") && !bare.contains("%{") {
        return "string";
    }
    let single = bare.strip_prefix("${").is_some_and(|rest| {
        let mut depth = 1usize;
        for (at, c) in rest.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return at + 1 == rest.len();
                    }
                }
                _ => {}
            }
        }
        false
    });
    if single { "expression" } else { "template" }
}

/// The value and type of the string `s` at `path` of the file `file`: a Terraform JSON string
/// is a template (`json_kind`), stored as the quoted JSON string when it is not a plain one, and
/// with its escapes (`$${`) undone when it is; a variable's `default` is a literal.
fn json_string(file: &str, path: &[Step], s: &str) -> (&'static str, String) {
    let literal =
        matches!(path, [Step::Key(v), _, Step::Key(d), ..] if v == "variable" && d == "default");
    if !terraform_json(file) || literal {
        return ("string", s.to_string());
    }
    match json_kind(s) {
        "string" => ("string", s.replace("$${", "${").replace("%%{", "%{")),
        kind => (kind, serde_json::to_string(s).unwrap_or_default()),
    }
}

/// Whether `object` is a Kubernetes `Secret`: an object with an `apiVersion` and `kind: Secret`.
fn is_secret(object: &Value) -> bool {
    object.get("apiVersion").is_some()
        && object.get("kind").and_then(Value::as_str) == Some("Secret")
}

/// The values of the file at `path` and what reading it came to. Every scalar but a null is a
/// row; a value of a Kubernetes `Secret`'s `data` or `stringData` (at the root of a document,
/// or an item of a `List`) keeps its key path, type and line only.
#[cfg(test)]
pub(crate) fn read(path: &str, src: &[u8]) -> (Outcome, Vec<Row>) {
    read_with(path, src, &BTreeSet::new())
}

/// `read` for the file at `path` against `sensitive`, the Terraform variables marked sensitive
/// that a `.tfvars` file's values are redacted for (`Sensitive::of`).
pub(crate) fn read_with(
    path: &str,
    src: &[u8],
    sensitive: &BTreeSet<String>,
) -> (Outcome, Vec<Row>) {
    let unreadable = |line, reason: String| (Outcome::Unreadable { line, reason }, Vec::new());
    if lockfile(path) {
        return (Outcome::Lockfile, Vec::new());
    }
    if src.len() > lang::MAX_BYTES {
        return unreadable(1, format!("larger than {} bytes", lang::MAX_BYTES));
    }
    if hcl::is_hcl(path) {
        return match hcl::read(path, src, sensitive) {
            Ok(rows) => {
                let redacted = rows.iter().filter(|r| r.value.is_none()).count();
                let outcome = Outcome::Read {
                    values: rows.len(),
                    redacted,
                    sensitive: sensitive.iter().cloned().collect(),
                };
                (outcome, rows)
            }
            Err((line, reason)) => unreadable(line, reason),
        };
    }
    let mut pending = Vec::new();
    let budget = (src.len())
        .saturating_mul(STORED_EXPANSION)
        .saturating_add(STORED_SLACK);
    let mut left = budget;
    let mut past: Option<(usize, String)> = None;
    let read = events::read(Syntax::of(path), src, |scalar| {
        if past.is_some() {
            return;
        }
        if scalar.document >= DOCUMENTS_MAX {
            past = Some((scalar.line, format!("more than {DOCUMENTS_MAX} documents")));
            return;
        }
        let (kind, mut value) = match scalar.value {
            Value::String(s) => json_string(path, scalar.path, s),
            Value::Number(n) => ("number", n.to_string()),
            Value::Bool(b) => ("boolean", b.to_string()),
            _ => return,
        };
        let cut = value.len() > VALUE_MAX;
        if cut {
            value.truncate(value.floor_char_boundary(VALUE_MAX));
        }
        let key = key_path(scalar.path);
        match left.checked_sub(key.len() + value.len()) {
            Some(rest) => left = rest,
            None => {
                past = Some((scalar.line, format!("values past {budget} bytes")));
                return;
            }
        }
        let secret = SecretPlace::of(scalar.path, path);
        let row = Row {
            key,
            value: Some(value),
            kind,
            line: scalar.line,
            cut,
        };
        pending.push(Pending {
            document: scalar.document,
            secret,
            row,
        });
    });
    let documents = match read {
        Ok(documents) => documents,
        Err((line, reason)) => return unreadable(line, reason),
    };
    if let Some((line, reason)) = past {
        return unreadable(line, reason);
    }
    if documents.len() > DOCUMENTS_MAX {
        return unreadable(1, format!("more than {DOCUMENTS_MAX} documents"));
    }
    let several = documents.len() > 1;
    let mut redacted = 0;
    let rows: Vec<Row> = (pending.into_iter())
        .map(
            |Pending {
                 document,
                 secret,
                 mut row,
             }| {
                let doc = &documents[document];
                let hidden = match secret {
                    SecretPlace::No => false,
                    SecretPlace::Root => is_secret(doc),
                    SecretPlace::Item(n) => doc["items"].get(n).is_some_and(is_secret),
                    SecretPlace::Variable(name) => {
                        doc["variable"][name.as_str()]["sensitive"] == Value::Bool(true)
                    }
                    SecretPlace::Tfvars(name) => {
                        sensitive.contains(&name) || sensitive.contains(ANY_VARIABLE)
                    }
                    SecretPlace::Parameter(name) => crate::iac::cloudformation::no_echo(doc, &name),
                    SecretPlace::SsmParameter(name) => doc["resource"]["aws_ssm_parameter"]
                        [name.as_str()]
                    .get("type")
                    .is_some_and(|t| ssm_value_is_secret(t.as_str())),
                };
                if hidden {
                    row.value = None;
                    row.cut = false;
                    redacted += 1;
                }
                if several {
                    row.key = prefixed(document, &row.key);
                }
                row
            },
        )
        .collect();
    let values = rows.len();
    let sensitive = sensitive.iter().cloned().collect();
    (
        Outcome::Read {
            values,
            redacted,
            sensitive,
        },
        rows,
    )
}

/// The key path of the scalar at `path` (ADR 0025), jq style: keys joined by dots, a list item
/// `[n]`, a key that is not a plain identifier `["..."]` with JSON escaping; `.` for a document
/// that is the scalar itself. A file of several documents puts `[document]` first (`prefixed`).
pub(crate) fn key_path(path: &[Step]) -> String {
    let mut out = String::new();
    for step in path {
        match step {
            Step::Index(n) => {
                let _ = write!(out, "[{n}]");
            }
            Step::Key(key) if plain(key) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            Step::Key(key) => {
                let quoted = serde_json::to_string(key).unwrap_or_default();
                let _ = write!(out, "[{quoted}]");
            }
        }
    }
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// `key`, a key path written without its document, with `[document]` first.
fn prefixed(document: usize, key: &str) -> String {
    match key {
        "." => format!("[{document}]"),
        _ if key.starts_with('[') => format!("[{document}]{key}"),
        _ => format!("[{document}].{key}"),
    }
}

/// An ASCII letter or `_`, then ASCII letters, digits or `_`: what jq reads as `.key`.
fn plain(key: &str) -> bool {
    let mut bytes = key.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Replaces the rows of the file at `path` with `rows`, each with its key path and value
/// lowercased beside it for the B-trees `hits` searches, and indexed by trigrams in
/// `config_search` for its substring rank.
pub(crate) fn store(tx: &Transaction, path: &str, rows: &[Row]) -> Result<()> {
    forget(tx, path)?;
    let mut insert = tx.prepare_cached(
        "INSERT INTO config_values (path, key, value, type, line, cut, folded_key, folded_value) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut index = tx.prepare_cached(
        "INSERT INTO config_search (rowid, folded_key, folded_value) VALUES (?1, ?2, ?3)",
    )?;
    for r in rows {
        let (key, value) = (
            r.key.to_lowercase(),
            r.value.as_deref().map(str::to_lowercase),
        );
        let line = r.line as i64;
        insert.execute(params![
            path, r.key, r.value, r.kind, line, r.cut, key, value
        ])?;
        index.execute(params![tx.last_insert_rowid(), key, value])?;
    }
    Ok(())
}

/// Drops the rows of the file at `path`.
pub(crate) fn forget(tx: &Transaction, path: &str) -> Result<()> {
    // An external-content FTS5 table is told the text it indexed, to drop it.
    tx.execute(
        "INSERT INTO config_search (config_search, rowid, folded_key, folded_value) \
         SELECT 'delete', id, folded_key, folded_value FROM config_values WHERE path = ?1",
        params![path],
    )?;
    tx.execute("DELETE FROM config_values WHERE path = ?1", params![path])?;
    Ok(())
}

/// The values of `db` (repository `identity`) whose lowercased value or key path is `needle`,
/// starts with it or holds it, as `search` hits: `<identity> <path>:<line> <key path> = <value>`,
/// a string quoted as JSON, a cut one followed by `(cut)` and a secret one `redacted`, ordered
/// by rank, then repository, path, line and key path. The B-trees on the lowercased key path
/// and value serve exact and prefix, and the trigram table `config_search` serves substring
/// (ADR 0025, #647); a query under three characters is answered by a scan, which is also what
/// the trigram path must agree with, byte for byte.
pub(crate) fn hits(identity: &str, db: &Connection, needle: &str) -> Result<Vec<Hit>> {
    hits_by(identity, db, needle, true)
}

/// The characters in a trigram: a query of fewer cannot be matched in `config_search`.
const TRIGRAM: usize = 3;

/// The substring filter by scan, over the folded key path and value (`?1` the needle).
const SCAN: &str = "instr(folded_key, ?1) > 0 OR instr(folded_value, ?1) > 0";

/// The substring filter through `config_search` (`?1` the needle, `?2` it as an FTS5 phrase).
/// The scan stays as a refilter: the table only narrows the rows looked at.
fn trigram_filter() -> String {
    format!("id IN (SELECT rowid FROM config_search WHERE config_search MATCH ?2) AND ({SCAN})")
}

/// `hits`, the substring rank answered by the trigram table when `indexed` and the query is
/// long enough, else by a scan.
fn hits_by(identity: &str, db: &Connection, needle: &str, indexed: bool) -> Result<Vec<Hit>> {
    const ROW: &str = "SELECT rowid, path, key, value, type, line, cut FROM config_values WHERE";
    // A key path or value that starts with `needle` sorts from it up to the least text above
    // all of them, as SQLite compares text by its UTF-8 bytes; with none, up to the end.
    let past = successor(needle);
    let (prefix, prefix_args): (&str, Vec<&str>) = match &past {
        Some(past) => (
            "folded_key >= ?1 AND folded_key < ?2 OR folded_value >= ?1 AND folded_value < ?2",
            vec![needle, past],
        ),
        None => ("folded_key >= ?1 OR folded_value >= ?1", vec![needle]),
    };
    // A trigram needs three characters; a NUL cannot be quoted into the match expression.
    let trigram = indexed && needle.chars().count() >= TRIGRAM && !needle.contains('\0');
    let phrase = format!("\"{}\"", needle.replace('"', "\"\""));
    let (substring, substring_args): (String, Vec<&str>) = if trigram {
        (trigram_filter(), vec![needle, &phrase])
    } else {
        (SCAN.to_string(), vec![needle])
    };
    let searches: [(u8, &str, &[&str]); 3] = [
        (0, "folded_key = ?1 OR folded_value = ?1", &[needle]),
        (1, prefix, &prefix_args),
        (2, &substring, &substring_args),
    ];
    let mut found: BTreeMap<i64, Hit> = BTreeMap::new();
    for (rank, filter, args) in searches {
        let mut statement = db.prepare(&format!("{ROW} {filter}"))?;
        let mut rows = statement.query(rusqlite::params_from_iter(args))?;
        while let Some(r) = rows.next()? {
            let rowid: i64 = r.get(0)?;
            if found.contains_key(&rowid) {
                continue;
            }
            let (path, key, value): (String, String, Option<String>) =
                (r.get(1)?, r.get(2)?, r.get(3)?);
            let (kind, line, cut): (String, i64, bool) = (r.get(4)?, r.get(5)?, r.get(6)?);
            let text = shown(identity, &path, line, &key, value.as_deref(), &kind, cut)?;
            found.insert(
                rowid,
                (rank, format!("{identity} {path} {line:010} {key}"), text),
            );
        }
    }
    Ok(found.into_values().collect())
}

/// `<identity> <path>:<line> <key path> = <value>`: a string quoted as JSON, an expression or a
/// template on one line, a cut value followed by `(cut)` and a secret one `redacted`.
fn shown(
    identity: &str,
    path: &str,
    line: i64,
    key: &str,
    value: Option<&str>,
    kind: &str,
    cut: bool,
) -> Result<String> {
    let shown = match (value, kind) {
        (None, _) => "redacted".to_string(),
        (Some(v), "string") => serde_json::to_string(v)?,
        (Some(v), "expression" | "template") => one_line(v),
        (Some(v), _) => v.to_string(),
    };
    let mark = if cut { " (cut)" } else { "" };
    Ok(format!("{identity} {path}:{line} {key} = {shown}{mark}"))
}

/// The values of the file at `path` of `db` (repository `identity`) whose key path is `block` or
/// lies under it (`block.` or `block[`), as `hits` shows them, by line and key path: the config
/// values of an IaC node. An empty `block` (an object that is the whole of its file) is every row.
pub(crate) fn under(
    identity: &str,
    db: &Connection,
    path: &str,
    block: &str,
) -> Result<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT key, value, type, line, cut FROM config_values WHERE path = ?1 \
         AND (?2 = '' OR key = ?2 OR substr(key, 1, length(?2) + 1) IN (?2 || '.', ?2 || '[')) \
         ORDER BY line, key",
    )?;
    let mut rows = statement.query(params![path, block])?;
    let mut lines = Vec::new();
    while let Some(r) = rows.next()? {
        let (key, value, kind): (String, Option<String>, String) =
            (r.get(0)?, r.get(1)?, r.get(2)?);
        let (line, cut): (i64, bool) = (r.get(3)?, r.get(4)?);
        lines.push(shown(
            identity,
            path,
            line,
            &key,
            value.as_deref(),
            &kind,
            cut,
        )?);
    }
    Ok(lines)
}

/// `text` on one line, as an answer has one line per match: a line break (and any other control
/// character, or a Unicode line separator) is written as an escape, so a multi-line expression
/// cannot pass for matches of its own.
fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c != '\t' && (c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')) => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// The least text above every text that starts with `prefix`, in code point order (the byte
/// order of UTF-8): `prefix` with its last character raised by one, after dropping each last
/// `char::MAX`; `None` when nothing is above them (an empty `prefix`, or one of `char::MAX` only).
fn successor(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        // Past the surrogates, which no `char` holds.
        let next = match last {
            '\u{D7FF}' => Some('\u{E000}'),
            c => char::from_u32(c as u32 + 1),
        };
        if let Some(next) = next {
            chars.push(next);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

/// The `meada status` line of the config files of `db`: how many were read and skipped (as
/// lockfiles, as unreadable) and how many values they hold and redact; `None` with no file.
pub(crate) fn status(db: &Connection) -> Result<Option<String>> {
    let mut statement = db.prepare("SELECT summary FROM config_files")?;
    let summaries = statement.query_map([], |r| r.get::<_, String>(0))?;
    let (mut read, mut lockfiles, mut unreadable, mut values, mut redacted) = (0, 0, 0, 0, 0);
    for summary in summaries {
        match serde_json::from_str(&summary?)? {
            Outcome::Read {
                values: v,
                redacted: r,
                ..
            } => {
                read += 1;
                values += v;
                redacted += r;
            }
            Outcome::Lockfile => lockfiles += 1,
            Outcome::Unreadable { .. } => unreadable += 1,
        }
    }
    Ok((read + lockfiles + unreadable > 0).then(|| {
        Msg::ConfigFiles {
            read,
            lockfiles,
            unreadable,
            values,
            redacted,
        }
        .text()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(path: &str, text: &str) -> Vec<(String, Option<String>, &'static str, usize)> {
        let (outcome, rows) = read(path, text.as_bytes());
        assert!(matches!(outcome, Outcome::Read { .. }), "{outcome:?}");
        (rows.into_iter())
            .map(|r| (r.key, r.value, r.kind, r.line))
            .collect()
    }

    fn keys(path: &str, text: &str) -> Vec<String> {
        rows(path, text).into_iter().map(|r| r.0).collect()
    }

    /// Every text that starts with the prefix sorts below its successor, even one where the
    /// last character is followed by `char::MAX`.
    #[test]
    fn the_successor_of_a_prefix_is_the_least_text_above_all_that_start_with_it() {
        let max = char::MAX;
        assert_eq!(successor("ab").as_deref(), Some("ac"));
        assert_eq!(successor(&format!("a{max}")).as_deref(), Some("b"));
        assert_eq!(successor("\u{D7FF}").as_deref(), Some("\u{E000}"));
        assert_eq!(successor(""), None);
        assert_eq!(successor(&format!("{max}{max}")), None);
        let past = successor("team").unwrap();
        for text in [
            "team",
            "team-b",
            &format!("team{max}"),
            &format!("team{max}{max}x"),
        ] {
            assert!(text.as_bytes() < past.as_bytes(), "{text:?}");
        }
        assert!("teamz".as_bytes() < past.as_bytes() && "tean".as_bytes() >= past.as_bytes());
    }

    #[test]
    fn a_tofu_file_shadows_the_terraform_file_of_its_name_in_its_directory() {
        let at = |paths: &[&str]| -> Vec<(String, PathBuf)> {
            (paths.iter())
                .map(|p| (p.to_string(), PathBuf::from(p)))
                .collect()
        };
        let kept = unshadowed(at(&[
            "a/main.tf",
            "a/main.tofu",
            "a/other.tf",
            "b/main.tf",
            "a/x.tf.json",
            "a/x.tofu.json",
            "a/y.tf.json",
            "a/main.tf.json",
            "a/main.tofu.json",
            "a/main.tfvars",
        ]));
        let kept: Vec<&str> = kept.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            kept,
            [
                "a/main.tofu",
                "a/other.tf",
                "b/main.tf",
                "a/x.tofu.json",
                "a/y.tf.json",
                "a/main.tofu.json",
                "a/main.tfvars"
            ]
        );
    }

    #[test]
    fn the_variable_a_sensitive_row_names_is_read_from_its_key_path() {
        assert_eq!(
            variable("variable.db_password.sensitive").as_deref(),
            Some("db_password")
        );
        assert_eq!(
            variable("variable[\"db-password\"].sensitive").as_deref(),
            Some("db-password")
        );
        assert_eq!(
            variable("variable[\"a\\\"b\"].sensitive").as_deref(),
            Some("a\"b")
        );
        assert_eq!(variable("output.x.sensitive"), None);
        assert_eq!(variable("variable.x.y.sensitive"), None);
        assert_eq!(variable("variable.x.default"), None);
    }

    #[test]
    fn a_terraform_json_string_is_a_template_unless_it_is_plain_or_a_variable_default() {
        for (text, kind) in [
            ("plain", "string"),
            ("a$${b}", "string"),
            ("${var.a}", "expression"),
            ("${ {a = 1}.a }", "expression"),
            ("${a}b}", "template"),
            ("${a} }", "template"),
            ("x-${a}", "template"),
            ("${a}${b}", "template"),
            ("%{if a}b%{endif}", "template"),
        ] {
            assert_eq!(json_kind(text), kind, "{text}");
        }
        let key = |names: &[&str]| -> Vec<Step> {
            names.iter().map(|n| Step::Key(n.to_string())).collect()
        };
        let default = key(&["variable", "x", "default"]);
        let other = key(&["resource", "a", "name"]);
        let file = "main.tf.json";
        assert_eq!(
            json_string(file, &default, "${a}"),
            ("string", "${a}".into())
        );
        assert_eq!(
            json_string(file, &other, "${a}b"),
            ("template", "\"${a}b\"".into())
        );
        assert_eq!(
            json_string(file, &other, "a$${b}"),
            ("string", "a${b}".into())
        );
        assert_eq!(
            json_string("a.json", &other, "${a}"),
            ("string", "${a}".into())
        );
    }

    #[test]
    fn an_expression_is_shown_on_one_line_whatever_it_holds() {
        assert_eq!(one_line("f({\n  a = 1\r\n})"), "f({\\n  a = 1\\r\\n})");
        assert_eq!(
            one_line("a\u{2028}b\u{0}c\u{85}d\te"),
            "a\\u{2028}b\\u{0}c\\u{85}d\te"
        );
        assert_eq!(one_line("var.env"), "var.env");
    }

    #[test]
    fn a_key_path_joins_plain_keys_by_dots_and_indexes_lists_from_zero() {
        let text = "spec:\n  containers:\n    - image: web\n      ports: [80, 443]\n";
        assert_eq!(
            keys("a.yaml", text),
            [
                "spec.containers[0].image",
                "spec.containers[0].ports[0]",
                "spec.containers[0].ports[1]"
            ]
        );
        assert_eq!(keys("a.json", "[{\"a\": 1}, [true]]"), ["[0].a", "[1][0]"]);
        assert_eq!(keys("a.yaml", "just text\n"), ["."]);
    }

    #[test]
    fn a_key_that_is_no_plain_identifier_is_quoted_with_json_escaping() {
        let text = concat!(
            "metadata:\n  annotations:\n    app.kubernetes.io/name: web\n",
            "env:\n  log-level: debug\n  _ok9: 1\n  9lives: 2\n  \"say \\\"hi\\\"\\n\": 3\n  \"\": 4\n  é: 5\n",
        );
        assert_eq!(
            keys("a.yaml", text),
            [
                "metadata.annotations[\"app.kubernetes.io/name\"]",
                "env[\"log-level\"]",
                "env._ok9",
                "env[\"9lives\"]",
                "env[\"say \\\"hi\\\"\\n\"]",
                "env[\"\"]",
                "env[\"é\"]",
            ]
        );
        assert_eq!(
            keys("a.json", "{\"a b\": {\"c\": [1]}}"),
            ["[\"a b\"].c[0]"]
        );
    }

    #[test]
    fn a_file_of_several_documents_starts_each_key_path_with_its_document() {
        let text = "a: 1\n---\n- x\n---\nplain\n---\n\"k-1\": 2\n";
        assert_eq!(
            keys("a.yaml", text),
            ["[0].a", "[1][0]", "[2]", "[3][\"k-1\"]"]
        );
    }

    #[test]
    fn each_row_holds_its_type_and_line_and_a_null_is_left_out() {
        let text =
            "name: web\nport: 8080\nratio: 0.5\non: true\nquoted: \"8080\"\nnone: null\nempty: ~\n";
        let s = |v: &str| Some(v.to_string());
        assert_eq!(
            rows("a.yaml", text),
            [
                ("name".into(), s("web"), "string", 1),
                ("port".into(), s("8080"), "number", 2),
                ("ratio".into(), s("0.5"), "number", 3),
                ("on".into(), s("true"), "boolean", 4),
                ("quoted".into(), s("8080"), "string", 5),
            ]
        );
    }

    #[test]
    fn a_value_past_the_ceiling_is_stored_cut_at_a_character_boundary() {
        let long = format!("{}é{}", "a".repeat(VALUE_MAX - 1), "b".repeat(10));
        let (_, rows) = read("a.yaml", format!("long: {long}\nshort: x\n").as_bytes());
        assert_eq!(
            rows[0].value.as_deref(),
            Some("a".repeat(VALUE_MAX - 1).as_str())
        );
        assert!(rows[0].cut);
        assert!(!rows[1].cut);
        let exact = "a".repeat(VALUE_MAX);
        let (_, rows) = read("a.yaml", format!("v: {exact}\n").as_bytes());
        assert_eq!(
            (rows[0].value.as_deref(), rows[0].cut),
            (Some(exact.as_str()), false)
        );
    }

    #[test]
    fn a_kubernetes_secret_keeps_its_data_and_string_data_redacted() {
        let text = concat!(
            "apiVersion: v1\nkind: ConfigMap\ndata:\n  plain: shown\n",
            "---\napiVersion: v1\nkind: Secret\nmetadata:\n  name: db\n",
            "data:\n  password: c2VjcmV0\nstringData:\n  port: 5432\n  user: admin\n",
            "---\napiVersion: v1\nkind: List\nitems:\n",
            "  - apiVersion: v1\n    kind: Secret\n    data:\n      token: dG9r\n",
            "  - apiVersion: v1\n    kind: ConfigMap\n    data:\n      open: yes\n",
            "---\nkind: Secret\ndata:\n  noversion: kept\n",
        );
        let (outcome, rows) = read("k8s.yaml", text.as_bytes());
        assert_eq!(
            outcome,
            Outcome::Read {
                values: 19,
                redacted: 4,
                sensitive: vec![]
            }
        );
        let hidden: Vec<_> = (rows.iter())
            .filter(|r| r.value.is_none())
            .map(|r| (r.key.as_str(), r.kind, r.line))
            .collect();
        assert_eq!(
            hidden,
            [
                ("[1].data.password", "string", 11),
                ("[1].stringData.port", "number", 13),
                ("[1].stringData.user", "string", 14),
                ("[2].items[0].data.token", "string", 22),
            ]
        );
        let shown = |key: &str| {
            rows.iter()
                .find(|r| r.key == key)
                .and_then(|r| r.value.clone())
        };
        assert_eq!(shown("[0].data.plain").as_deref(), Some("shown"));
        assert_eq!(shown("[1].metadata.name").as_deref(), Some("db"));
        assert_eq!(shown("[2].items[1].data.open").as_deref(), Some("yes"));
        assert_eq!(shown("[3].data.noversion").as_deref(), Some("kept"));
    }

    #[test]
    fn a_lockfile_a_file_that_does_not_parse_and_one_past_a_ceiling_are_skipped() {
        for lockfile in [
            "package-lock.json",
            "web/pnpm-lock.yaml",
            "src/App/packages.lock.json",
            "npm-shrinkwrap.json",
        ] {
            assert_eq!(
                read(lockfile, b"{}"),
                (Outcome::Lockfile, vec![]),
                "{lockfile}"
            );
        }
        let (outcome, rows) = read(
            "chart/templates/a.yaml",
            b"a: 1\n{{- if .Values.on }}\nb: 2\n",
        );
        assert!(
            matches!(outcome, Outcome::Unreadable { line: 2, .. }),
            "{outcome:?}"
        );
        assert!(rows.is_empty());
        let documents = "---\na: 1\n".repeat(DOCUMENTS_MAX + 1);
        let (outcome, _) = read("many.yaml", documents.as_bytes());
        assert!(
            matches!(&outcome, Outcome::Unreadable { reason, .. } if reason.contains("documents")),
            "{outcome:?}"
        );
        let (outcome, _) = read("many.yaml", "---\na: 1\n".repeat(DOCUMENTS_MAX).as_bytes());
        assert_eq!(
            outcome,
            Outcome::Read {
                values: DOCUMENTS_MAX,
                redacted: 0,
                sensitive: vec![]
            }
        );
        let large = format!("\"{}\"", "a".repeat(lang::MAX_BYTES));
        assert!(matches!(
            read("big.json", large.as_bytes()).0,
            Outcome::Unreadable { line: 1, .. }
        ));
    }

    /// Long keys nested deep, each repeated in the key path of every scalar below them, would
    /// store many times the file: the file stops at its stored budget.
    #[test]
    fn a_file_whose_key_paths_store_past_the_budget_is_unreadable() {
        let key = "k".repeat(4000);
        let deep = format!("{{\"{key}\": ").repeat(20);
        let leaves: Vec<String> = (0..2000).map(|n| format!("\"v{n}\": 1")).collect();
        let text = format!("{deep}{{{}}}{}", leaves.join(", "), "}".repeat(20));
        let (outcome, rows) = read("a.json", text.as_bytes());
        assert!(
            matches!(&outcome, Outcome::Unreadable { reason, .. } if reason.contains("past")),
            "{outcome:?}"
        );
        assert!(rows.is_empty());
    }

    /// The trigram table and the scan answer every query shape alike over many values: exact,
    /// prefix, substring (in a value, in a key path, with characters a pattern would read
    /// specially, in several scripts) and under three characters; the first two do not use the
    /// table, the substring ones do, and each finds something.
    #[test]
    fn the_trigram_index_answers_every_query_shape_as_the_scan_does() {
        const WORDS: [&str; 9] = [
            "orders",
            "billing",
            "Éclair",
            "straße",
            "say \"hi\"",
            "100%_done",
            "a*b",
            "İstanbul",
            "日本語のキュー",
        ];
        let mut teams = serde_json::Map::new();
        for n in 0..1500 {
            let value = format!("queue-{}-{}", n % 37, WORDS[n % WORDS.len()]);
            let team = teams.entry(format!("team{}", n % 7)).or_insert(Value::Null);
            if team.is_null() {
                *team = Value::Object(Default::default());
            }
            team[format!("svc{n}")] = Value::String(value);
        }
        let text = serde_json::to_string(&Value::Object(teams)).unwrap();
        let (outcome, rows) = read("big.json", text.as_bytes());
        assert!(
            matches!(outcome, Outcome::Read { values: 1500, .. }),
            "{outcome:?}"
        );
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(crate::index::SCHEMA).unwrap();
        let tx = db.transaction().unwrap();
        store(&tx, "big.json", &rows).unwrap();
        tx.commit().unwrap();

        let queries: [(&str, bool); 22] = [
            ("queue-3-orders", true),
            ("queue-3", true),
            ("team3.svc10", true),
            ("ders", true),
            ("vc12", true),
            ("ord", true),
            ("éclair", true),
            ("straße", true),
            ("\"hi", true),
            ("%_", true),
            ("*b", true),
            ("i\u{307}stan", true),
            ("キュー", true),
            ("zzzz", false),
            ("", true),
            ("q", true),
            ("ab", false),
            ("é", true),
            ("日本", true),
            ("0", true),
            ("1", true),
            ("[3]", false),
        ];
        for (query, found) in queries {
            let query = query.to_lowercase();
            let indexed = hits_by("acme/app", &db, &query, true).unwrap();
            let scanned = hits_by("acme/app", &db, &query, false).unwrap();
            assert_eq!(indexed, scanned, "{query:?}");
            assert_eq!(!indexed.is_empty(), found, "{query:?}");
        }
        // A file stored again leaves nothing of its old rows in the table.
        let kept = &rows[..10];
        let tx = db.transaction().unwrap();
        store(&tx, "big.json", kept).unwrap();
        tx.commit().unwrap();
        db.execute(
            "INSERT INTO config_search (config_search, rank) VALUES ('integrity-check', 1)",
            [],
        )
        .unwrap();
        let indexed = hits_by("acme/app", &db, "queue", true).unwrap();
        assert_eq!(indexed.len(), 10);
        assert_eq!(indexed, hits_by("acme/app", &db, "queue", false).unwrap());
        // The substring rank asks the trigram table (a scan would not name it).
        let mut statement = db
            .prepare(&format!(
                "EXPLAIN QUERY PLAN SELECT rowid FROM config_values WHERE {}",
                trigram_filter()
            ))
            .unwrap();
        let plan: Vec<String> = (statement.query_map(["ord", "\"ord\""], |r| r.get(3)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(plan.iter().any(|d| d.contains("config_search")), "{plan:?}");
        // Emptying the table with the rows, as a new identity does, leaves it consistent.
        db.execute_batch(
            "DELETE FROM config_values; INSERT INTO config_search (config_search) VALUES ('delete-all');",
        )
        .unwrap();
        db.execute(
            "INSERT INTO config_search (config_search, rank) VALUES ('integrity-check', 1)",
            [],
        )
        .unwrap();
    }
}
