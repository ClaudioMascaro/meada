//! The index: one SQLite file in `<repo>/.meada/`, holding spans and identities but no
//! source text (ADR 0004).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::Result;
use crate::contract::{self, Level, Link, Names, Site};
use crate::decisions::Stale;
use crate::lang::{self, Facts};
use crate::resolve::{Code, Resolution, Resolver, Target};

/// Bump on any change to `SCHEMA` or to the facts a pack stores: an index with another
/// version is deleted and rebuilt.
const SCHEMA_VERSION: i64 = 9;

/// `meta` holds the repository identity and the BLAKE3 of the packs' config files.
/// `files.mtime` (nanoseconds) and `size` are the stat seen when the file was hashed; `mtime`
/// is NULL when it was too recent to trust. `files.facts` is what the pack parsed, as JSON
/// (names, specifiers and byte ranges, no source text); `files.resolved` is the BLAKE3 of the
/// file's resolved symbol and call rows. `calls.target` holds the one definition an `exact` or
/// `inferred` call reaches; an `ambiguous` call has no target and lists every definition that
/// fits in `candidates`. An unresolved consumer's `contracts` row keeps the `method` it calls
/// with, when readable, and the literal `fragments` of its URL path as a JSON array (NULL when
/// none); they let `Engine::open_items` list the routes it may reach.
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
CREATE TABLE contracts (
    role TEXT NOT NULL, contract TEXT, site TEXT NOT NULL, via TEXT, path TEXT NOT NULL,
    method TEXT, fragments TEXT
);
CREATE INDEX contracts_path ON contracts (path);
";

/// A file modified this close to a refresh may change again within the same mtime tick, so
/// its mtime is not trusted on the next refresh and the file is hashed again.
const RACY: Duration = Duration::from_secs(2);

/// Where the index of the repository at `root` lives.
pub fn path(root: &Path) -> PathBuf {
    root.join(".meada/index.db")
}

/// What `status` reports about one repository's index, read without writing.
pub enum State {
    Missing,
    OtherSchema,
    Indexed { files: i64 },
}

pub fn state(root: &Path) -> State {
    let path = path(root);
    if !path.is_file() {
        return State::Missing;
    }
    let Ok(db) = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return State::OtherSchema;
    };
    let version = db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0));
    if version.ok() != Some(SCHEMA_VERSION) {
        return State::OtherSchema;
    }
    match db.query_row("SELECT count(*) FROM files", [], |r| r.get(0)) {
        Ok(files) => State::Indexed { files },
        Err(_) => State::OtherSchema,
    }
}

/// Opens `<root>/.meada/index.db`, creating `.meada/` with a `.gitignore` of `*`.
pub fn open(root: &Path) -> Result<Connection> {
    let dir = root.join(".meada");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(".gitignore"), "*\n")?;
    let path = path(root);
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
    level: Option<Level>,
    start: i64,
    end: i64,
    candidates: Vec<String>,
}

/// The symbol and call rows of one file, before they are written.
#[derive(Serialize)]
struct Rows {
    symbols: Vec<(String, i64, i64)>,
    calls: Vec<Resolved>,
}

/// Brings the index up to date with the files a language pack handles under `root`: tracked
/// and untracked, minus what `.gitignore` excludes. Only files whose mtime or size differ
/// from the index are hashed, and only those whose hash differs are parsed. Every file's
/// calls are then resolved against the new facts, and a file's symbol and call rows are
/// rewritten when the file changed or its rows now differ: that covers importers of a changed
/// file's exports, re-export chains, name inference, config changes and inline route
/// handlers renamed by a mount elsewhere alike. Contracts are extracted again for the whole
/// repository, since constants and client wrappers reach across files. Deleted files leave
/// the index. The result equals a refresh of an empty index over the same files.
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
                 DELETE FROM calls; DELETE FROM candidates; DELETE FROM contracts;",
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
        tx.execute("DELETE FROM contracts WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
    }
    for p in &changed {
        write_file(&tx, p)?;
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
            let language = lang::pack(pack);
            let modules = language.modules(&found)?;
            Ok((pack, Resolver::new(files, modules, language.receiver())))
        })
        .collect::<Result<BTreeMap<usize, Resolver>>>()?;

    // Contracts, from every file. Values are not kept in the index, so the files whose
    // values a kind reads are parsed again, until no kind misses one.
    // ponytail: every contract is extracted again on any change; track which files each
    // contract read if this shows up in profiles.
    let mut reparsed: BTreeMap<String, Facts> = BTreeMap::new();
    let found = loop {
        let full: BTreeMap<&str, &Facts> = changed
            .iter()
            .map(|p| (p.path.as_str(), &p.facts))
            .chain(reparsed.iter().map(|(path, facts)| (path.as_str(), facts)))
            .collect();
        let mut found = Vec::new();
        let mut misses = Vec::new();
        for resolver in resolvers.values() {
            let code = Code::new(resolver, &full);
            for kind in contract::KINDS {
                found.extend(kind.extract(&code));
            }
            misses.extend(code.misses().into_iter().map(String::from));
        }
        if misses.is_empty() {
            break found;
        }
        for path in misses {
            let src = fs::read(root.join(&path))?;
            let facts = lang::pack(packs[path.as_str()]).parse(&path, &src)?;
            reparsed.insert(path, facts);
        }
    };
    let names = Names::new(&found);

    // ponytail: every file's calls are resolved again on any change, in memory; only the
    // rows that differ are written. A reverse-import graph can narrow the set if resolution
    // shows up in profiles.
    for (&path, &facts) in &all {
        let rows = resolve_file(identity, path, facts, &resolvers[&packs[path]], &names);
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
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        for (id, start, end) in &rows.symbols {
            tx.execute(
                "INSERT INTO symbols VALUES (?1, ?2, ?3, ?4)",
                params![id, path, start, end],
            )?;
        }
        for row in &rows.calls {
            tx.execute(
                "INSERT INTO calls VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    row.id,
                    row.caller,
                    row.target,
                    row.level.map(Level::name),
                    path,
                    row.start,
                    row.end
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

    tx.execute("DELETE FROM contracts", [])?;
    let mut calls: HashMap<&str, Vec<(String, String)>> = HashMap::new();
    let mut rows = BTreeSet::new();
    for f in &found {
        let (role, path, site) = match &f.site {
            Site::Def { path, name }
            | Site::Inline {
                path, def: name, ..
            } => {
                let name = names.def(path, name).unwrap_or_else(|| name.clone());
                ("provider", path, format!("{identity}::{path}::{name}"))
            }
            Site::File(path) => ("provider", path, format!("{identity}::{path}")),
            Site::Call { path, index } => {
                let ids = calls
                    .entry(path)
                    .or_insert_with(|| call_ids(identity, path, all[path.as_str()], &names));
                let Some((_, id)) = ids.get(*index) else {
                    continue;
                };
                ("consumer", path, id.clone())
            }
        };
        let fragments = (!f.fragments.is_empty()).then(|| serde_json::to_string(&f.fragments));
        rows.insert((
            role,
            site,
            &f.contract,
            &f.via,
            path,
            &f.method,
            fragments.transpose()?,
        ));
    }
    for (role, site, contract, via, path, method, fragments) in rows {
        tx.execute(
            "INSERT INTO contracts VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![role, contract, site, via, path, method, fragments],
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

/// Replaces a changed file's row and facts. Its symbols and calls are written once resolved,
/// its contracts with the repository's, and links are derived from contracts when queried.
fn write_file(tx: &Transaction, p: &Parsed) -> Result<()> {
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

/// The caller and id of each call in a file, in source order: `<caller>::<callee>#<ordinal>`.
/// A call inside an inline function that folds belongs to the enclosing definition.
fn call_ids(identity: &str, path: &str, facts: &Facts, names: &Names) -> Vec<(String, String)> {
    let base = format!("{identity}::{path}");
    let mut ordinals: HashMap<(String, &str), usize> = HashMap::new();
    facts
        .calls
        .iter()
        .map(|call| {
            let scope = names.scope(path, &call.scope);
            let caller = if scope.is_empty() {
                base.clone()
            } else {
                format!("{base}::{scope}")
            };
            let ordinal = ordinals
                .entry((scope, call.callee.as_str()))
                .and_modify(|n| *n += 1)
                .or_insert(0);
            let id = format!("{caller}::{}#{ordinal}", call.callee);
            (caller, id)
        })
        .collect()
}

/// The symbol and call rows of one file, in source order.
fn resolve_file<'a>(
    identity: &str,
    path: &'a str,
    facts: &'a Facts,
    resolver: &Resolver<'a>,
    names: &Names,
) -> Rows {
    let id = |t: &Target| {
        let name = names.def(t.path, t.name).unwrap_or_else(|| t.name.into());
        format!("{identity}::{}::{name}", t.path)
    };
    let symbols = facts
        .defs
        .iter()
        .filter_map(|def| {
            let name = names.def(path, &def.name)?;
            let id = format!("{identity}::{path}::{name}");
            Some((id, def.range.start as i64, def.range.end as i64))
        })
        .collect();
    let calls = facts
        .calls
        .iter()
        .zip(call_ids(identity, path, facts, names))
        .map(|(call, (caller, call_id))| {
            let (target, level, candidates) = match resolver.call(path, call) {
                Resolution::Exact(t) => (Some(id(&t)), Some(Level::Exact), Vec::new()),
                Resolution::Inferred(t) => (Some(id(&t)), Some(Level::Inferred), Vec::new()),
                Resolution::Ambiguous(ts) => {
                    (None, Some(Level::Ambiguous), ts.iter().map(id).collect())
                }
                Resolution::Unresolved => (None, None, Vec::new()),
            };
            Resolved {
                id: call_id,
                caller,
                target,
                level,
                start: call.range.start as i64,
                end: call.range.end as i64,
                candidates,
            }
        })
        .collect();
    Rows { symbols, calls }
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
    pub(crate) providers: Vec<Provider>,
    pub(crate) consumers: Vec<Consumer>,
    pub(crate) links: Vec<Link>,
    /// The repository's decisions that do not apply, each with its cause (`meada export` only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) stale_decisions: Vec<Stale>,
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
    pub(crate) level: Option<Level>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) candidates: Vec<String>,
    pub(crate) span: Span,
}

/// A symbol that serves a contract.
#[derive(Serialize)]
pub(crate) struct Provider {
    pub(crate) contract: String,
    pub(crate) site: String,
    /// The path a gateway serves the route at, when `[serve]` rewrites it (`meada export`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) gateway: Option<String>,
}

/// A call site that uses a contract; `contract` is `None` while it is unresolved. `via` says
/// how it names the provider's repository, such as `host:api.example.com` or `env:API_URL`.
/// An unresolved one also keeps what could be read of its URL (see `Found`); the export leaves
/// that out.
#[derive(Serialize)]
pub(crate) struct Consumer {
    pub(crate) contract: Option<String>,
    pub(crate) site: String,
    pub(crate) via: Option<String>,
    #[serde(skip)]
    pub(crate) method: Option<String>,
    #[serde(skip)]
    pub(crate) fragments: Vec<String>,
}

/// The index content of the repository attached as `schema`, every list sorted. Links are
/// left empty: they span repositories, so the engine adds them.
pub fn export(db: &Connection, schema: &str, identity: &str) -> Result<Export> {
    let files = db
        .prepare(&format!(
            "SELECT path, hash FROM {schema}.files ORDER BY path"
        ))?
        .query_map([], |r| {
            Ok(File {
                path: r.get(0)?,
                hash: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let symbols = db
        .prepare(&format!(
            "SELECT id, path, start, end FROM {schema}.symbols ORDER BY id, path, start, end"
        ))?
        .query_map([], |r| {
            Ok(Symbol {
                id: r.get(0)?,
                span: span(r, 1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut candidates: HashMap<String, Vec<String>> = HashMap::new();
    for row in db
        .prepare(&format!(
            "SELECT call, target FROM {schema}.candidates ORDER BY call, target"
        ))?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
    {
        let (call, target) = row?;
        candidates.entry(call).or_default().push(target);
    }
    let calls = db
        .prepare(&format!(
            "SELECT id, caller, target, level, path, start, end FROM {schema}.calls \
             ORDER BY id, path, start, end"
        ))?
        .query_map([], |r| {
            let id: String = r.get(0)?;
            Ok(CallSite {
                candidates: candidates.remove(&id).unwrap_or_default(),
                id,
                caller: r.get(1)?,
                target: r.get(2)?,
                level: r
                    .get::<_, Option<String>>(3)?
                    .as_deref()
                    .and_then(Level::parse),
                span: span(r, 4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let providers = db
        .prepare(&format!(
            "SELECT contract, site FROM {schema}.contracts WHERE role = 'provider' \
             ORDER BY contract, site"
        ))?
        .query_map([], |r| {
            Ok(Provider {
                contract: r.get(0)?,
                site: r.get(1)?,
                gateway: None,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let consumers = db
        .prepare(&format!(
            "SELECT contract, site, via, method, fragments FROM {schema}.contracts \
             WHERE role = 'consumer' ORDER BY site, contract, via"
        ))?
        .query_map([], |r| {
            let fragments: Option<String> = r.get(4)?;
            Ok(Consumer {
                contract: r.get(0)?,
                site: r.get(1)?,
                via: r.get(2)?,
                method: r.get(3)?,
                // ponytail: written by `refresh` only, so a bad value is an index to rebuild.
                fragments: fragments
                    .and_then(|f| serde_json::from_str(&f).ok())
                    .unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Export {
        repository: identity.to_string(),
        files,
        symbols,
        calls,
        providers,
        consumers,
        links: Vec::new(),
        stale_decisions: Vec::new(),
    })
}

fn span(row: &rusqlite::Row, at: usize) -> rusqlite::Result<Span> {
    Ok(Span {
        path: row.get(at)?,
        start: row.get(at + 1)?,
        end: row.get(at + 2)?,
    })
}
