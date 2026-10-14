//! The index: one SQLite file in `<repo>/.meada/`, holding spans and identities but no
//! source text (ADR 0004).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::Result;
use crate::lang::{self, Facts};
use crate::resolve::{Resolution, Resolver, Target};

/// Bump on any change to `SCHEMA`: an index with another version is deleted and rebuilt.
const SCHEMA_VERSION: i64 = 2;

/// `calls.target` holds the one definition an `exact` or `inferred` call reaches; an
/// `ambiguous` call has no target and lists every definition that fits in `candidates`.
const SCHEMA: &str = "
CREATE TABLE files (path TEXT PRIMARY KEY, hash TEXT NOT NULL);
CREATE TABLE symbols (id TEXT NOT NULL, path TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL);
CREATE TABLE calls (
    id TEXT NOT NULL, caller TEXT NOT NULL, target TEXT, level TEXT,
    path TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL
);
CREATE TABLE candidates (call TEXT NOT NULL, target TEXT NOT NULL);
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

/// One source file a language pack parsed.
struct Parsed {
    path: String,
    hash: String,
    pack: usize,
    facts: Facts,
}

/// Replaces the index content with the files a language pack handles under `root`: tracked
/// and untracked, minus what `.gitignore` excludes.
pub fn rebuild(db: &mut Connection, root: &Path, identity: &str) -> Result<()> {
    let mut files: Vec<(String, PathBuf, usize)> = WalkBuilder::new(root)
        .require_git(false)
        // The repository's own ignore files only, never those of directories above it.
        .parents(false)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|entry| {
            let rel = relative(root, entry.path())?;
            let pack = lang::pack_for(&rel)?;
            Some((rel, entry.into_path(), pack))
        })
        .collect();
    files.sort();
    let parsed = files
        .into_iter()
        .map(|(path, abs, pack)| {
            let src = fs::read(&abs)?;
            let facts = lang::pack(pack).parse(&path, &src)?;
            let hash = blake3::hash(&src).to_hex().to_string();
            Ok(Parsed {
                path,
                hash,
                pack,
                facts,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Calls resolve among the files of their own pack.
    let mut groups: BTreeMap<usize, BTreeMap<&str, &Facts>> = BTreeMap::new();
    for p in &parsed {
        groups
            .entry(p.pack)
            .or_default()
            .insert(p.path.as_str(), &p.facts);
    }
    let resolvers = groups
        .iter()
        .map(|(&pack, files)| {
            let configs: Vec<(&str, Vec<u8>)> = lang::pack(pack)
                .config_files()
                .iter()
                .filter_map(|name| Some((*name, fs::read(root.join(name)).ok()?)))
                .collect();
            let configs: Vec<(&str, &[u8])> =
                configs.iter().map(|(n, c)| (*n, c.as_slice())).collect();
            let modules = lang::pack(pack).modules(&configs)?;
            Ok((pack, Resolver::new(files, modules)))
        })
        .collect::<Result<BTreeMap<usize, Resolver>>>()?;

    let id = |t: &Target| format!("{identity}::{}::{}", t.path, t.name);
    let tx = db.transaction()?;
    tx.execute_batch(
        "DELETE FROM files; DELETE FROM symbols; DELETE FROM calls; DELETE FROM candidates;",
    )?;
    for p in &parsed {
        let rel = p.path.as_str();
        tx.execute("INSERT INTO files VALUES (?1, ?2)", params![rel, p.hash])?;
        let base = format!("{identity}::{rel}");
        for def in &p.facts.defs {
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
        let mut ordinals: HashMap<(&str, &str), usize> = HashMap::new();
        for call in &p.facts.calls {
            let ordinal = ordinals
                .entry((call.scope.as_str(), call.callee.as_str()))
                .and_modify(|n| *n += 1)
                .or_insert(0);
            let caller = if call.scope.is_empty() {
                base.clone()
            } else {
                format!("{base}::{}", call.scope)
            };
            let call_id = format!("{caller}::{}#{ordinal}", call.callee);
            let (target, level, candidates) = match resolvers[&p.pack].call(rel, call) {
                Resolution::Exact(t) => (Some(id(&t)), Some("exact"), Vec::new()),
                Resolution::Inferred(t) => (Some(id(&t)), Some("inferred"), Vec::new()),
                Resolution::Ambiguous(ts) => (None, Some("ambiguous"), ts),
                Resolution::Unresolved => (None, None, Vec::new()),
            };
            tx.execute(
                "INSERT INTO calls VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    call_id,
                    caller,
                    target,
                    level,
                    rel,
                    call.range.start as i64,
                    call.range.end as i64
                ],
            )?;
            for candidate in &candidates {
                tx.execute(
                    "INSERT INTO candidates VALUES (?1, ?2)",
                    params![call_id, id(candidate)],
                )?;
            }
        }
    }
    tx.commit()?;
    Ok(())
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
pub struct Export {
    repository: String,
    files: Vec<File>,
    symbols: Vec<Symbol>,
    calls: Vec<CallSite>,
}

#[derive(Serialize)]
struct File {
    path: String,
    hash: String,
}

#[derive(Serialize)]
struct Span {
    path: String,
    start: i64,
    end: i64,
}

#[derive(Serialize)]
struct Symbol {
    id: String,
    span: Span,
}

#[derive(Serialize)]
struct CallSite {
    id: String,
    caller: String,
    target: Option<String>,
    level: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<String>,
    span: Span,
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
    let mut candidates: HashMap<String, Vec<String>> = HashMap::new();
    for row in db
        .prepare("SELECT call, target FROM candidates ORDER BY call, target")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
    {
        let (call, target) = row?;
        candidates.entry(call).or_default().push(target);
    }
    let calls = db
        .prepare(
            "SELECT id, caller, target, level, path, start, end FROM calls \
             ORDER BY id, path, start, end",
        )?
        .query_map([], |r| {
            let id: String = r.get(0)?;
            Ok(CallSite {
                candidates: candidates.remove(&id).unwrap_or_default(),
                id,
                caller: r.get(1)?,
                target: r.get(2)?,
                level: r.get(3)?,
                span: span(r, 4)?,
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
