//! The index: one SQLite file in `<repo>/.meada/`, holding spans and identities but no
//! source text (ADR 0004).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::Result;
use crate::lang::{self, Facts};
use crate::resolve::{Resolution, Resolver, Target};

/// Bump on any change to `SCHEMA` or to the facts a pack stores: an index with another
/// version is deleted and rebuilt.
const SCHEMA_VERSION: i64 = 3;

/// `meta` holds the repository identity and the BLAKE3 of the packs' config files.
/// `files.mtime` (nanoseconds) and `size` are the stat seen when the file was hashed; `mtime`
/// is NULL when it was too recent to trust. `files.facts` is what the pack parsed, as JSON
/// (names, specifiers and byte ranges, no source text); `files.resolved` is the BLAKE3 of the
/// file's resolved call rows. `calls.target` holds the one definition an `exact` or
/// `inferred` call reaches; an `ambiguous` call has no target and lists every definition that
/// fits in `candidates`.
const SCHEMA: &str = "
CREATE TABLE meta (identity TEXT NOT NULL, configs TEXT NOT NULL);
CREATE TABLE files (
    path TEXT PRIMARY KEY, hash TEXT NOT NULL, mtime INTEGER, size INTEGER NOT NULL,
    facts TEXT NOT NULL, resolved TEXT NOT NULL
);
CREATE TABLE symbols (id TEXT NOT NULL, path TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL);
CREATE INDEX symbols_path ON symbols (path);
CREATE TABLE calls (
    id TEXT NOT NULL, caller TEXT NOT NULL, target TEXT, level TEXT,
    path TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL
);
CREATE INDEX calls_path ON calls (path);
CREATE TABLE candidates (call TEXT NOT NULL, target TEXT NOT NULL);
CREATE INDEX candidates_call ON candidates (call);
";

/// A file modified this close to a refresh may change again within the same mtime tick, so
/// its mtime is not trusted on the next refresh and the file is hashed again.
const RACY: Duration = Duration::from_secs(2);

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

/// What the index knows of a file.
struct Stored {
    hash: String,
    mtime: Option<i64>,
    size: i64,
    resolved: String,
}

/// A file whose content differs from the index, parsed again.
struct Parsed {
    path: String,
    hash: String,
    mtime: Option<i64>,
    size: i64,
    facts: Facts,
}

/// One call site as resolved, before it is written.
#[derive(Serialize)]
struct Resolved {
    id: String,
    caller: String,
    target: Option<String>,
    level: Option<&'static str>,
    start: i64,
    end: i64,
    candidates: Vec<String>,
}

/// Brings the index up to date with the files a language pack handles under `root`: tracked
/// and untracked, minus what `.gitignore` excludes. Only files whose mtime or size differ
/// from the index are hashed, and only those whose hash differs are parsed. Every file's
/// calls are then resolved against the new facts, and a file's call rows are rewritten when
/// the file changed or its calls now resolve differently: that covers importers of a changed
/// file's exports, re-export chains, name inference and config changes alike. Deleted files
/// leave the index. The result equals a refresh of an empty index over the same files.
pub fn refresh(db: &Connection, root: &Path, identity: &str) -> Result<()> {
    let now = SystemTime::now();
    let tx = db.unchecked_transaction()?;
    let known = tx
        .query_row("SELECT identity, configs FROM meta", [], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .optional()?;
    let known_configs = match known {
        Some((id, configs)) if id == identity => Some(configs),
        // Another identity renames every symbol: start over.
        _ => {
            tx.execute_batch(
                "DELETE FROM meta; DELETE FROM files; DELETE FROM symbols; \
                 DELETE FROM calls; DELETE FROM candidates;",
            )?;
            None
        }
    };
    let stored: HashMap<String, Stored> = tx
        .prepare("SELECT path, hash, mtime, size, resolved FROM files")?
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                Stored {
                    hash: r.get(1)?,
                    mtime: r.get(2)?,
                    size: r.get(3)?,
                    resolved: r.get(4)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let walked = walk(root);
    let packs: BTreeMap<&str, usize> = walked.iter().map(|(p, _, k)| (p.as_str(), *k)).collect();
    let mut changed = Vec::new();
    for (path, abs, pack) in &walked {
        let stat = fs::metadata(abs)?;
        let size = stat.len() as i64;
        let modified = stat.modified()?;
        let mtime = modified
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_nanos() as i64);
        let known = stored.get(path);
        if known.is_some_and(|k| k.mtime.is_some() && k.mtime == mtime && k.size == size) {
            continue;
        }
        let mtime = mtime.filter(|_| modified + RACY < now);
        let src = fs::read(abs)?;
        let hash = blake3::hash(&src).to_hex().to_string();
        if known.is_some_and(|k| k.hash == hash) {
            tx.execute(
                "UPDATE files SET mtime = ?2, size = ?3 WHERE path = ?1",
                params![path, mtime, size],
            )?;
            continue;
        }
        let facts = lang::pack(*pack).parse(path, &src)?;
        changed.push(Parsed {
            path: path.clone(),
            hash,
            mtime,
            size,
            facts,
        });
    }
    let deleted: Vec<&String> = stored
        .keys()
        .filter(|p| !packs.contains_key(p.as_str()))
        .collect();

    // Config files, read whole on every refresh: they are few and small.
    let mut configs: BTreeMap<usize, Vec<(&str, Vec<u8>)>> =
        packs.values().map(|&pack| (pack, Vec::new())).collect();
    let mut digest = blake3::Hasher::new();
    for (&pack, found) in &mut configs {
        for &name in lang::pack(pack).config_files() {
            if let Ok(src) = fs::read(root.join(name)) {
                digest.update(format!("{pack}\0{name}\0{}\0", src.len()).as_bytes());
                digest.update(&src);
                found.push((name, src));
            }
        }
    }
    let digest = digest.finalize().to_hex().to_string();
    if changed.is_empty() && deleted.is_empty() && known_configs.as_deref() == Some(&digest) {
        tx.commit()?;
        return Ok(());
    }

    for path in &deleted {
        forget_calls(&tx, path)?;
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
    }
    for p in &changed {
        write_file(&tx, identity, p)?;
    }

    // The facts of every file: parsed now, or kept from the index.
    let changed_paths: HashSet<&str> = changed.iter().map(|p| p.path.as_str()).collect();
    let mut kept: Vec<(String, Facts)> = Vec::new();
    for row in tx
        .prepare("SELECT path, facts FROM files")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
    {
        let (path, facts) = row?;
        if !changed_paths.contains(path.as_str()) {
            kept.push((path, serde_json::from_str(&facts)?));
        }
    }
    let all: BTreeMap<&str, &Facts> = changed
        .iter()
        .map(|p| (p.path.as_str(), &p.facts))
        .chain(kept.iter().map(|(path, facts)| (path.as_str(), facts)))
        .collect();

    // Calls resolve among the files of their own pack.
    let mut groups: BTreeMap<usize, BTreeMap<&str, &Facts>> = BTreeMap::new();
    for (&path, &facts) in &all {
        groups.entry(packs[path]).or_default().insert(path, facts);
    }
    let resolvers = groups
        .iter()
        .map(|(&pack, files)| {
            let found: Vec<(&str, &[u8])> = configs[&pack]
                .iter()
                .map(|(n, c)| (*n, c.as_slice()))
                .collect();
            Ok((
                pack,
                Resolver::new(files, lang::pack(pack).modules(&found)?),
            ))
        })
        .collect::<Result<BTreeMap<usize, Resolver>>>()?;

    // ponytail: every file's calls are resolved again on any change, in memory; only the
    // rows that differ are written. A reverse-import graph can narrow the set if resolution
    // shows up in profiles.
    for (&path, &facts) in &all {
        let rows = resolve_file(identity, path, facts, &resolvers[&packs[path]]);
        let resolved = blake3::hash(&serde_json::to_vec(&rows)?)
            .to_hex()
            .to_string();
        let before = (!changed_paths.contains(path))
            .then(|| stored.get(path).map(|s| s.resolved.as_str()))
            .flatten();
        if before == Some(resolved.as_str()) {
            continue;
        }
        forget_calls(&tx, path)?;
        for row in &rows {
            tx.execute(
                "INSERT INTO calls VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    row.id, row.caller, row.target, row.level, path, row.start, row.end
                ],
            )?;
            for candidate in &row.candidates {
                tx.execute(
                    "INSERT INTO candidates VALUES (?1, ?2)",
                    params![row.id, candidate],
                )?;
            }
        }
        tx.execute(
            "UPDATE files SET resolved = ?2 WHERE path = ?1",
            params![path, resolved],
        )?;
    }
    tx.execute("DELETE FROM meta", [])?;
    tx.execute(
        "INSERT INTO meta VALUES (?1, ?2)",
        params![identity, digest],
    )?;
    tx.commit()?;
    Ok(())
}

/// Makes the next refresh hash `path` again, whatever its mtime and size say.
pub fn forget(db: &Connection, path: &str) -> Result<()> {
    db.execute(
        "UPDATE files SET mtime = NULL WHERE path = ?1",
        params![path],
    )?;
    Ok(())
}

/// The files a language pack handles under `root`, sorted, with their pack.
fn walk(root: &Path) -> Vec<(String, PathBuf, usize)> {
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
    files
}

/// Replaces a changed file's row, facts and symbols. Its calls are written once resolved.
fn write_file(tx: &Transaction, identity: &str, p: &Parsed) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO files VALUES (?1, ?2, ?3, ?4, ?5, '')",
        params![
            p.path,
            p.hash,
            p.mtime,
            p.size,
            serde_json::to_string(&p.facts)?
        ],
    )?;
    tx.execute("DELETE FROM symbols WHERE path = ?1", params![p.path])?;
    for def in &p.facts.defs {
        tx.execute(
            "INSERT INTO symbols VALUES (?1, ?2, ?3, ?4)",
            params![
                format!("{identity}::{}::{}", p.path, def.name),
                p.path,
                def.range.start as i64,
                def.range.end as i64
            ],
        )?;
    }
    Ok(())
}

fn forget_calls(tx: &Transaction, path: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM candidates WHERE call IN (SELECT id FROM calls WHERE path = ?1)",
        params![path],
    )?;
    tx.execute("DELETE FROM calls WHERE path = ?1", params![path])?;
    Ok(())
}

/// The call rows of one file, in source order.
fn resolve_file<'a>(
    identity: &str,
    path: &'a str,
    facts: &'a Facts,
    resolver: &Resolver<'a>,
) -> Vec<Resolved> {
    let id = |t: &Target| format!("{identity}::{}::{}", t.path, t.name);
    let base = format!("{identity}::{path}");
    let mut ordinals: HashMap<(&str, &str), usize> = HashMap::new();
    facts
        .calls
        .iter()
        .map(|call| {
            let ordinal = ordinals
                .entry((call.scope.as_str(), call.callee.as_str()))
                .and_modify(|n| *n += 1)
                .or_insert(0);
            let caller = if call.scope.is_empty() {
                base.clone()
            } else {
                format!("{base}::{}", call.scope)
            };
            let (target, level, candidates) = match resolver.call(path, call) {
                Resolution::Exact(t) => (Some(id(&t)), Some("exact"), Vec::new()),
                Resolution::Inferred(t) => (Some(id(&t)), Some("inferred"), Vec::new()),
                Resolution::Ambiguous(ts) => (None, Some("ambiguous"), ts.iter().map(id).collect()),
                Resolution::Unresolved => (None, None, Vec::new()),
            };
            Resolved {
                id: format!("{caller}::{}#{ordinal}", call.callee),
                caller,
                target,
                level,
                start: call.range.start as i64,
                end: call.range.end as i64,
                candidates,
            }
        })
        .collect()
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
    pub(crate) level: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidates: Vec<String>,
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
