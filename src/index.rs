//! The index: one SQLite file in `<repo>/.meada/`, holding spans and identities but no
//! source text (ADR 0004).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::{Result, ts};

/// Bump on any change to `SCHEMA`: an index with another version is deleted and rebuilt.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE files (path TEXT PRIMARY KEY, hash TEXT NOT NULL);
CREATE TABLE symbols (id TEXT NOT NULL, path TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL);
CREATE TABLE calls (
    id TEXT NOT NULL, caller TEXT NOT NULL, target TEXT,
    path TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL
);
";

/// Opens `<root>/.meada/index.db`, creating `.meada/` with a `.gitignore` of `*`.
pub fn open(root: &Path) -> Result<Connection> {
    let dir = root.join(".meada");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(".gitignore"), "*\n")?;
    let path = dir.join("index.db");
    let db = Connection::open(&path)?;
    // An unreadable file counts as a mismatch: `.meada/` is Meada's own directory.
    let version = db
        .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .ok();
    if version == Some(SCHEMA_VERSION) {
        return Ok(db);
    }
    drop(db);
    let _ = fs::remove_file(&path);
    let db = Connection::open(&path)?;
    db.execute_batch(SCHEMA)?;
    db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(db)
}

/// Replaces the index content with the TypeScript files under `root`: tracked and untracked,
/// minus what `.gitignore` excludes.
pub fn rebuild(db: &mut Connection, root: &Path, identity: &str) -> Result<()> {
    let mut files: Vec<(String, PathBuf)> = WalkBuilder::new(root)
        .require_git(false)
        // The repository's own ignore files only, never those of directories above it.
        .parents(false)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|entry| Some((relative(root, entry.path())?, entry.into_path())))
        .filter(|(rel, _)| ts::language(rel).is_some())
        .collect();
    files.sort();

    let tx = db.transaction()?;
    tx.execute_batch("DELETE FROM files; DELETE FROM symbols; DELETE FROM calls;")?;
    for (rel, abs) in &files {
        let src = fs::read(abs)?;
        tx.execute(
            "INSERT INTO files VALUES (?1, ?2)",
            params![rel, blake3::hash(&src).to_hex().as_str()],
        )?;
        let language = ts::language(rel).expect("filtered above");
        let (defs, calls) = ts::parse(&language, &src)?;
        let base = format!("{identity}::{rel}");
        for def in &defs {
            tx.execute(
                "INSERT INTO symbols VALUES (?1, ?2, ?3, ?4)",
                params![
                    format!("{base}::{}", def.name),
                    rel,
                    def.range.start as i64,
                    def.range.end as i64
                ],
            )?;
        }
        let names: HashSet<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        let mut ordinals: HashMap<(&str, &str), usize> = HashMap::new();
        for call in &calls {
            let ordinal = ordinals
                .entry((call.scope.as_str(), call.callee.as_str()))
                .and_modify(|n| *n += 1)
                .or_insert(0);
            let caller = if call.scope.is_empty() {
                base.clone()
            } else {
                format!("{base}::{}", call.scope)
            };
            let target = resolve(&names, &call.scope, &call.callee).map(|q| format!("{base}::{q}"));
            tx.execute(
                "INSERT INTO calls VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    format!("{caller}::{}#{ordinal}", call.callee),
                    caller,
                    target,
                    rel,
                    call.range.start as i64,
                    call.range.end as i64
                ],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// A plain identifier resolves to the definition visible from `scope`: the innermost
/// enclosing function first, the top level last.
fn resolve(names: &HashSet<&str>, scope: &str, callee: &str) -> Option<String> {
    if callee.contains(['.', '(']) {
        return None;
    }
    let mut scope = Some(scope);
    while let Some(s) = scope {
        let candidate = if s.is_empty() {
            callee.to_string()
        } else {
            format!("{s}.{callee}")
        };
        if names.contains(candidate.as_str()) {
            return Some(candidate);
        }
        scope = (!s.is_empty()).then(|| s.rsplit_once('.').map_or("", |(parent, _)| parent));
    }
    None
}

/// `path` relative to `root` with `/` separators; `None` for non-UTF-8 paths.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = path
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect();
    Some(parts?.join("/"))
}

#[derive(Serialize)]
pub(crate) struct Export {
    pub(crate) repository: String,
    pub(crate) files: Vec<File>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) calls: Vec<CallSite>,
}

#[derive(Serialize)]
pub(crate) struct File {
    pub(crate) path: String,
    pub(crate) hash: String,
}

#[derive(Serialize)]
pub(crate) struct Span {
    pub(crate) path: String,
    pub(crate) start: i64,
    pub(crate) end: i64,
}

#[derive(Serialize)]
pub(crate) struct Symbol {
    pub(crate) id: String,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct CallSite {
    pub(crate) id: String,
    pub(crate) caller: String,
    pub(crate) target: Option<String>,
    pub(crate) span: Span,
}

/// The index content, every list sorted.
pub fn export(db: &Connection, identity: &str) -> Result<Export> {
    let files = db
        .prepare("SELECT path, hash FROM files ORDER BY path")?
        .query_map([], |r| {
            Ok(File {
                path: r.get(0)?,
                hash: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let symbols = db
        .prepare("SELECT id, path, start, end FROM symbols ORDER BY id, path, start, end")?
        .query_map([], |r| {
            Ok(Symbol {
                id: r.get(0)?,
                span: span(r, 1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let calls = db
        .prepare(
            "SELECT id, caller, target, path, start, end FROM calls \
             ORDER BY id, path, start, end",
        )?
        .query_map([], |r| {
            Ok(CallSite {
                id: r.get(0)?,
                caller: r.get(1)?,
                target: r.get(2)?,
                span: span(r, 3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Export {
        repository: identity.to_string(),
        files,
        symbols,
        calls,
    })
}

fn span(row: &rusqlite::Row, at: usize) -> rusqlite::Result<Span> {
    Ok(Span {
        path: row.get(at)?,
        start: row.get(at + 1)?,
        end: row.get(at + 2)?,
    })
}
