//! The index: one SQLite file in `<repo>/.meada/`, holding spans and identities but no
//! source text (ADR 0004).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Serialize, Serializer};

use crate::Result;
use crate::community::{self, Community};
use crate::config::Serve;
use crate::contract::{self, Level, Link, Names, Role, Site};
use crate::decisions::Stale;
use crate::document::{self, Format, Summary};
use crate::entry::{self, EntryKind, EntryPoint};
use crate::lang::{self, Facts};
use crate::package::{self, Config, Dependency, Hop, Import, ImportCall, Manifest, Package};
use crate::resolve::{Code, Exported, Resolution, Resolver, Target};
use crate::shape::{Shape, Want};
use crate::trace::Process;

/// Bump on any change to `SCHEMA`, to the facts a pack stores or to a document's or a
/// manifest's stored `summary`: an index with another version is deleted and rebuilt.
const SCHEMA_VERSION: i64 = 20;

/// `meta` holds the repository identity and the BLAKE3 of the packs' config files.
/// `files.mtime` (nanoseconds) and `size` are the stat seen when the file was hashed; `mtime`
/// is NULL when it was too recent to trust. `files.facts` is what the pack parsed, as JSON
/// (names, specifiers and byte ranges, no source text); `files.resolved` is the BLAKE3 of the
/// file's resolved symbol and call rows. `calls.target` holds the one definition an `exact` or
/// `inferred` call reaches; an `ambiguous` call has no target and lists every definition that
/// fits in `candidates`. An unresolved consumer's `contracts` row keeps the `method` it calls
/// with, when readable, and the literal `fragments` of its URL path as a JSON array (NULL when
/// none); they let `Engine::open_items` list the routes it may reach. `contracts.shape` is the
/// shape the code gives a side (`shape::Shape` as JSON: its source and field paths, spec #23),
/// such as the fields an HTTP consumer reads from the parsed response; NULL when none. A RabbitMQ `binding` row
/// keeps the queue's channel in `contract`, the exchange in `via` and the pattern in `method`;
/// a `publish` row (a publish to a named exchange) has no `contract`, the exchange in `via` and
/// the routing key in `method`. `documents` holds each provider document (spec #21) with the
/// same stat and hash as `files`, and its `summary` (`document::Summary` as JSON: format,
/// version, title, routes with their shapes, servers, channel sides with theirs, and problems,
/// derived from the bytes alone); a document that could not be read keeps its row, so its hash
/// is cached too. `manifests` holds each `package.json` (spec #22) the same way, its `summary`
/// being `package::Summary` as JSON (name, version, dependency entries and the entry fields
/// `exports`, `types`, `module` and `main`), read from the bytes alone; a manifest without a
/// name keeps its row too. A `tsconfig.json` has a row there as well, its `summary` being
/// `package::Tsconfig` (`out_dir`, `root_dir`), or `null` when the file is invalid. An `import`
/// row of `contracts` is a call through a binding imported from a bare specifier (`via`), naming `method` of it. A `shape`
/// row (issue #175) is the shape of the module-level constant or type `method` a file exports,
/// read for the view `via` (`shape::Want`), kept only in a repository with a named manifest.
/// `exports` holds what each file exports (`resolve::Exported`): a `name` with the `file` and
/// `target` definition, constant or type it names (`target`
/// NULL for a whole file, `file` NULL for nothing the repository holds), or, with `name` NULL,
/// an `export * from` the file `file`. Both are rewritten with the contracts, and read at query
/// time to resolve imports of a package into another repository (spec #22). `communities`
/// holds one row per member of each community (spec #31): its name and the member's symbol
/// identity, recomputed from the `calls` rows whenever the code rows change.
/// `entry_points` holds the repository's doors (spec #31, `entry`): the `identity`, the `kind`
/// (`route`, `export`, `channel` or `program`), the `label` that names its process and the `site` the process starts
/// from; rewritten with the contracts.
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
    method TEXT, fragments TEXT, shape TEXT
);
CREATE INDEX contracts_path ON contracts (path);
CREATE TABLE documents (
    path TEXT PRIMARY KEY, hash TEXT NOT NULL, mtime INTEGER, size INTEGER NOT NULL,
    summary TEXT NOT NULL
);
CREATE TABLE manifests (
    path TEXT PRIMARY KEY, hash TEXT NOT NULL, mtime INTEGER, size INTEGER NOT NULL,
    summary TEXT NOT NULL
);
CREATE TABLE exports (path TEXT NOT NULL, name TEXT, file TEXT, target TEXT);
CREATE TABLE entry_points (identity TEXT NOT NULL, kind TEXT NOT NULL, label TEXT NOT NULL, site TEXT NOT NULL);
CREATE TABLE communities (name TEXT NOT NULL, member TEXT NOT NULL);
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

/// What the index knows of a document: its hash and the stat seen when it was hashed.
struct Stamp {
    hash: String,
    mtime: Option<i64>,
    size: i64,
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
/// the index. The result equals a refresh of an empty index over the same files. Stored facts
/// that do not parse (a change that missed its `SCHEMA_VERSION` bump) rebuild the index.
///
/// Documents (spec #21) are refreshed first, the same way, when `serve` is declared; without
/// a `[serve]` table the repository has none. They hold no code, so an edited document changes
/// no code row: `Http::declare` reads them at query time.
pub fn refresh(db: &Connection, root: &Path, identity: &str, serve: &Serve) -> Result<()> {
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
                 DELETE FROM calls; DELETE FROM candidates; DELETE FROM contracts; \
                 DELETE FROM documents; DELETE FROM manifests; DELETE FROM exports; \
                 DELETE FROM entry_points; DELETE FROM communities;",
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

    let all = walk(root);
    refresh_stamped(
        &tx,
        "documents",
        &documents_found(root, &all, serve),
        now,
        |path, src| Ok(serde_json::to_string(&document::read(path, src))?),
    )?;
    refresh_stamped(
        &tx,
        "manifests",
        &manifests_found(&all),
        now,
        |path, src| match package::is_tsconfig(path) {
            true => Ok(serde_json::to_string(&package::read_tsconfig(src))?),
            false => Ok(serde_json::to_string(&package::read(src))?),
        },
    )?;
    // The packages decide which files keep the shapes of what they export (issue #175).
    let manifest_rows: Vec<(String, String)> = tx
        .prepare("SELECT path, summary FROM manifests ORDER BY path")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let walked: Vec<(&String, &PathBuf, usize)> = (all.iter())
        .filter_map(|(path, abs)| Some((path, abs, lang::pack_for(path)?)))
        .collect();
    let packs: BTreeMap<&str, usize> = walked.iter().map(|(p, _, k)| (p.as_str(), *k)).collect();
    let mut changed = Vec::new();
    for &(path, abs, pack) in &walked {
        let (size, mtime, trusted) = stat(abs, now)?;
        let known = stored.get(path);
        if known.is_some_and(|k| k.mtime.is_some() && k.mtime == mtime && k.size == size) {
            continue;
        }
        let mtime = mtime.filter(|_| trusted);
        let src = fs::read(abs)?;
        let hash = blake3::hash(&src).to_hex().to_string();
        if known.is_some_and(|k| k.hash == hash) {
            tx.execute(
                "UPDATE files SET mtime = ?2, size = ?3 WHERE path = ?1",
                params![path, mtime, size],
            )?;
            continue;
        }
        let facts = lang::pack(pack).parse(path, &src)?;
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
    // An edited `package.json` or `tsconfig.json` changes which files keep shapes (`surface`),
    // so it extracts again. Folding them in also forces that on an index of this version written
    // before the `shape` rows and constant `exports` targets existed (issue #175), which the
    // schema golden does not cover.
    for (path, summary) in &manifest_rows {
        digest.update(format!("{path}\0{summary}\0").as_bytes());
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
    let rows = tx
        .prepare("SELECT path, facts FROM files")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
    let kept: Option<Vec<(String, Facts)>> = rows
        .into_iter()
        .filter(|(path, _)| !changed_paths.contains(path.as_str()))
        .map(|(path, facts)| Some((path, serde_json::from_str(&facts).ok()?)))
        .collect();
    let Some(kept) = kept else {
        // Facts this version cannot read, stored before a change that missed its version
        // bump: rebuild, as for another version. Without the identity, the next pass starts
        // over and keeps nothing.
        drop(tx);
        db.execute("DELETE FROM meta", [])?;
        return refresh(db, root, identity, serve);
    };
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
            Ok((pack, Resolver::new(files, modules, language)))
        })
        .collect::<Result<BTreeMap<usize, Resolver>>>()?;
    // The files an import of one of the repository's packages reaches: only they keep shapes.
    let surface = {
        let (manifests, tsconfigs) = read_manifests(manifest_rows)?;
        let exported: Vec<Exported> = resolvers.values().flat_map(Resolver::exported).collect();
        let files: Vec<&str> = all.keys().copied().collect();
        package::surface(&manifests, &tsconfigs, &exported, &files)
    };

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
        for (&pack, resolver) in &resolvers {
            let code = Code::new(resolver, &full, lang::pack(pack));
            for kind in contract::KINDS {
                found.extend(kind.extract(&code));
            }
            if !surface.is_empty() {
                found.extend(package::exported_shapes(&code, &surface));
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
    // The symbols and the targets of every call, for the entry points.
    let mut symbols: HashSet<String> = HashSet::new();
    let mut called: HashSet<String> = HashSet::new();
    let mut order: Vec<(&&str, &&Facts)> = all.iter().collect();
    if REVERSED_WALK.get() {
        order.reverse();
    }
    for (&path, &facts) in order {
        let rows = resolve_file(identity, path, facts, &resolvers[&packs[path]], &names);
        symbols.extend(rows.symbols.iter().map(|(id, ..)| id.clone()));
        for call in &rows.calls {
            // A self-call is no caller: recursion keeps an exported function a door.
            let reached = call.target.iter().chain(&call.candidates);
            called.extend(reached.filter(|t| **t != call.caller).cloned());
        }
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
    // One row per side; the shapes of the Founds of one side (a site reached through several
    // wrapper paths) merge into it.
    let mut rows: BTreeMap<_, Option<Shape>> = BTreeMap::new();
    let mut providers: Vec<entry::Side> = Vec::new();
    let mut consumers: Vec<entry::Side> = Vec::new();
    for f in &found {
        let (path, site) = match &f.site {
            // An inline definition no kind names folds into the one enclosing it, or into the
            // file at the top level.
            Site::Def { path, name }
            | Site::Inline {
                path, def: name, ..
            } => match names.scope(path, name) {
                name if name.is_empty() => (path, format!("{identity}::{path}")),
                name => (path, format!("{identity}::{path}::{name}")),
            },
            Site::File(path) => (path, format!("{identity}::{path}")),
            Site::Call { path, index } => {
                let ids = calls
                    .entry(path)
                    .or_insert_with(|| call_ids(identity, path, all[path.as_str()], &names));
                let Some((_, id)) = ids.get(*index) else {
                    continue;
                };
                (path, id.clone())
            }
        };
        let role = f.role.name();
        if let (Role::Provider, Some(contract)) = (f.role, &f.contract) {
            providers.push(entry::Side {
                site: site.clone(),
                contract: contract.clone(),
            });
        }
        if let (Role::Consumer, Some(contract)) = (f.role, &f.contract) {
            consumers.push(entry::Side {
                site: site.clone(),
                contract: contract.clone(),
            });
        }
        let fragments = (!f.fragments.is_empty()).then(|| serde_json::to_string(&f.fragments));
        let key = (
            role,
            site,
            &f.contract,
            &f.via,
            path,
            &f.method,
            fragments.transpose()?,
        );
        let shape = rows.entry(key).or_default();
        match (shape.as_mut(), &f.shape) {
            (Some(shape), Some(more)) => shape.merge(more),
            (None, more) => *shape = more.clone(),
            (_, None) => {}
        }
    }
    for ((role, site, contract, via, path, method, fragments), shape) in rows {
        let shape = shape.as_ref().map(serde_json::to_string).transpose()?;
        tx.execute(
            "INSERT INTO contracts VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![role, contract, site, via, path, method, fragments, shape],
        )?;
    }
    tx.execute("DELETE FROM exports", [])?;
    for e in resolvers.values().flat_map(Resolver::exported) {
        tx.execute(
            "INSERT INTO exports VALUES (?1, ?2, ?3, ?4)",
            params![e.path, e.name, e.file, e.target],
        )?;
    }
    let exported: Vec<Exported> = resolvers.values().flat_map(Resolver::exported).collect();
    let programs: Vec<entry::Program> = (all.iter())
        .flat_map(|(&path, &facts)| {
            (facts.programs.iter()).map(move |def| entry::Program {
                file: path.to_string(),
                def: def.clone(),
            })
        })
        .collect();
    let local = entry::Local {
        identity,
        files: &all,
        names: &names,
        symbols: &symbols,
        called: &called,
        exported: &exported,
        providers: &providers,
        consumers: &consumers,
        programs: &programs,
    };
    tx.execute("DELETE FROM entry_points", [])?;
    for e in entry::find(&local) {
        tx.execute(
            "INSERT INTO entry_points VALUES (?1, ?2, ?3, ?4)",
            params![e.identity, e.kind.name(), e.label, e.site],
        )?;
    }
    write_communities(&tx)?;
    tx.execute("DELETE FROM meta", [])?;
    tx.execute(
        "INSERT INTO meta VALUES (?1, ?2)",
        params![identity, digest],
    )?;
    tx.commit()?;
    Ok(())
}

/// Rewrites the `communities` rows from the stored calls at `exact` or `inferred` between two
/// symbols, each named after its members' parent directories.
fn write_communities(tx: &Transaction) -> Result<()> {
    let dirs: HashMap<String, String> = tx
        .prepare("SELECT id, path FROM symbols")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let calls: Vec<(String, String)> = tx
        .prepare(
            "SELECT caller, target FROM calls WHERE level IN ('exact', 'inferred') \
             AND target IS NOT NULL",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    // A top-level caller is a file, not a symbol: no directory, no node.
    let calls = calls.iter().map(|(a, b)| (a.as_str(), b.as_str()));
    let dir = |id: &str| {
        let path = dirs.get(id)?;
        Some(
            path.rsplit_once('/')
                .map_or(".", |(dir, _)| dir)
                .to_string(),
        )
    };
    tx.execute("DELETE FROM communities", [])?;
    for c in community::communities(calls, dir) {
        for member in &c.members {
            tx.execute(
                "INSERT INTO communities VALUES (?1, ?2)",
                params![c.name, member],
            )?;
        }
    }
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

/// The documents of a repository (path and absolute path): those `walk` found by file name,
/// then each path `[serve] documents` lists that is a file, ignored by `.gitignore` or not. A
/// listed path that is not a file, or that goes through a symbolic link, is no document;
/// `document::report` says so. Without a `[serve]` table there are none.
fn documents_found(
    root: &Path,
    walked: &[(String, PathBuf)],
    serve: &Serve,
) -> Vec<(String, PathBuf)> {
    if !serve.declared {
        return Vec::new();
    }
    let mut found: Vec<_> = (walked.iter())
        .filter(|(path, _)| document::is_document(path))
        .cloned()
        .collect();
    for path in &serve.documents {
        if found.iter().any(|(p, _)| p == path) {
            continue;
        }
        if let Some(abs) = plain_file(root, path) {
            found.push((path.clone(), abs));
        }
    }
    found
}

/// The file at `path` under `root`, when no segment of `path` is a symbolic link: like the
/// walk, a listed path never leads out of the repository.
fn plain_file(root: &Path, path: &str) -> Option<PathBuf> {
    let mut abs = root.to_path_buf();
    let mut file = false;
    for segment in path.split('/') {
        abs.push(segment);
        let kind = fs::symlink_metadata(&abs).ok()?.file_type();
        if kind.is_symlink() {
            return None;
        }
        file = kind.is_file();
    }
    file.then_some(abs)
}

/// The `package.json` and `tsconfig.json` files of a repository (path and absolute path), but
/// those under a `node_modules` directory, which the walk finds when `.gitignore` does not list
/// it.
fn manifests_found(walked: &[(String, PathBuf)]) -> Vec<(String, PathBuf)> {
    let is_manifest = |path: &str| {
        let mut segments = path.split('/').rev();
        matches!(segments.next(), Some("package.json" | "tsconfig.json"))
            && !segments.any(|s| s == "node_modules")
    };
    (walked.iter())
        .filter(|(path, _)| is_manifest(path))
        .cloned()
        .collect()
}

/// Brings `table` (`documents` or `manifests`, both stamped like `files`) up to date with
/// `found`, the files on disk (path and absolute path), as `refresh` does `files`: stat, then
/// hash, then `summarize` only what changed. A stored row not found, or whose file cannot be
/// statted or read, leaves the table.
fn refresh_stamped(
    tx: &Transaction,
    table: &str,
    found: &[(String, PathBuf)],
    now: SystemTime,
    summarize: impl Fn(&str, &[u8]) -> Result<String>,
) -> Result<()> {
    let stored: HashMap<String, Stamp> = tx
        .prepare(&format!("SELECT path, hash, mtime, size FROM {table}"))?
        .query_map([], |r| {
            let (hash, mtime, size) = (r.get(1)?, r.get(2)?, r.get(3)?);
            Ok((r.get(0)?, Stamp { hash, mtime, size }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut seen = HashSet::new();
    for (path, abs) in found {
        // A file that vanished or cannot be read since the walk has no row, and a stored one
        // leaves the table; it never fails the refresh.
        let Ok((size, mtime, trusted)) = stat(abs, now) else {
            continue;
        };
        seen.insert(path.as_str());
        let known = stored.get(path);
        if known.is_some_and(|k| k.mtime.is_some() && k.mtime == mtime && k.size == size) {
            continue;
        }
        let mtime = mtime.filter(|_| trusted);
        let Ok(src) = fs::read(abs) else {
            seen.remove(path.as_str());
            continue;
        };
        let hash = blake3::hash(&src).to_hex().to_string();
        if known.is_some_and(|k| k.hash == hash) {
            tx.execute(
                &format!("UPDATE {table} SET mtime = ?2, size = ?3 WHERE path = ?1"),
                params![path, mtime, size],
            )?;
            continue;
        }
        let summary = summarize(path, &src)?;
        tx.execute(
            &format!("INSERT OR REPLACE INTO {table} VALUES (?1, ?2, ?3, ?4, ?5)"),
            params![path, hash, mtime, size, summary],
        )?;
    }
    for path in stored.keys().filter(|p| !seen.contains(p.as_str())) {
        tx.execute(
            &format!("DELETE FROM {table} WHERE path = ?1"),
            params![path],
        )?;
    }
    Ok(())
}

/// The size and mtime (nanoseconds) of the file at `abs`, and whether that mtime is old
/// enough, by `RACY`, to be trusted on the next refresh.
fn stat(abs: &Path, now: SystemTime) -> Result<(i64, Option<i64>, bool)> {
    let stat = fs::metadata(abs)?;
    let modified = stat.modified()?;
    let mtime = modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_nanos() as i64);
    Ok((stat.len() as i64, mtime, modified + RACY < now))
}

thread_local! {
    static REVERSED_WALK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` with this thread's indexing in reverse order (determinism test, spec #31): the
/// walk hands its files over in reverse path order, so they are parsed in it, and the rows of
/// `symbols`, `calls` and `candidates` are inserted in reverse path order. The in-memory maps
/// stay keyed by path. Indexing runs on one thread, so there is no worker count to change.
/// Test-only: no command, option or setting reaches it.
#[doc(hidden)]
pub fn with_reversed_walk<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            REVERSED_WALK.set(self.0);
        }
    }
    let _restore = Restore(REVERSED_WALK.replace(true));
    f()
}

/// Every file under `root` but what `.gitignore` excludes, sorted (reversed under
/// [`with_reversed_walk`]).
fn walk(root: &Path) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = WalkBuilder::new(root)
        .require_git(false)
        // The repository's own ignore files only, never those of directories above it.
        .parents(false)
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|entry| Some((relative(root, entry.path())?, entry.into_path())))
        .collect();
    files.sort();
    if REVERSED_WALK.get() {
        files.reverse();
    }
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
#[cfg_attr(test, derive(Default))]
pub(crate) struct Export {
    pub(crate) repository: String,
    pub(crate) files: Vec<File>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) calls: Vec<CallSite>,
    pub(crate) providers: Vec<Provider>,
    pub(crate) consumers: Vec<Consumer>,
    /// The RabbitMQ bindings the repository declares (spec #20), neither side of a channel.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) bindings: Vec<Binding>,
    /// The provider documents (spec #21), in path order; those read are exported, with
    /// their contracts.
    #[serde(
        serialize_with = "documents_read",
        skip_serializing_if = "no_document_read"
    )]
    pub(crate) documents: Vec<Document>,
    /// The RabbitMQ publishes to a named exchange (spec #20), producers of the queues the
    /// bindings route them to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) publishes: Vec<Publish>,
    /// The `package.json` files read (spec #22), in path order. Not exported as such:
    /// `Package::declare` turns the named ones into `packages` and providers, and
    /// `package::consume` the dependency entries that name a package of the workspace into
    /// `dependencies` and consumers.
    #[serde(skip)]
    pub(crate) manifests: Vec<Manifest>,
    /// The `tsconfig.json` files read (spec #22), in path order: `package::import` maps a
    /// package's build output back to source through them.
    #[serde(skip)]
    pub(crate) tsconfigs: Vec<Config>,
    /// The packages the repository provides, by name, then directory.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) packages: Vec<Package>,
    /// The dependency entries naming a package of the workspace, by manifest, field, name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) dependencies: Vec<Dependency>,
    /// The calls through a binding imported from a bare specifier, by site. Not exported:
    /// `package::import` resolves those that name a package of the workspace.
    #[serde(skip)]
    pub(crate) import_calls: Vec<ImportCall>,
    /// What each file exports (the `exports` table), sorted, for `package::import`.
    #[serde(skip)]
    pub(crate) exported: Vec<Exported>,
    /// The shapes kept for what each file exports (`shape` rows, issue #175), for
    /// `package::shapes`.
    #[serde(skip)]
    pub(crate) shapes: Vec<package::ExportedShape>,
    /// The imports resolved into a package of the workspace, by call site, then target.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) imports: Vec<Import>,
    /// The repository's communities (spec #31), by name, each with its sorted members.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) communities: Vec<Community>,
    pub(crate) links: Vec<Link>,
    /// The repository's entry points (spec #31), sorted by identity. Not exported as such:
    /// `processes` is what the export shows.
    #[serde(skip)]
    pub(crate) entry_points: Vec<EntryPoint>,
    /// The process of each entry point, in entry point order (`meada export` only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) processes: Vec<Process>,
    /// The repository's decisions that do not apply, each with its cause (`meada export` only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) stale_decisions: Vec<Stale>,
}

/// Puts `providers` back in the order `export` reads them in, after a kind's `declare` added
/// some.
pub(crate) fn sort_providers(providers: &mut [Provider]) {
    providers.sort_by(|a, b| (&a.contract, &a.site).cmp(&(&b.contract, &b.site)));
}

/// Puts `consumers` back in the order `export` reads them in, after a kind's `declare` added
/// some.
pub(crate) fn sort_consumers(consumers: &mut [Consumer]) {
    consumers.sort_by(|a, b| (&a.site, &a.contract, &a.via).cmp(&(&b.site, &b.contract, &b.via)));
}

impl Export {
    /// The document whose route `site` is, when `site` is a route only a document of this
    /// repository lists: `<repository>::<document path>::<METHOD> <path>`.
    pub(crate) fn document_of(&self, site: &str) -> Option<&str> {
        let rest = site.strip_prefix(&self.repository)?.strip_prefix("::")?;
        let (path, _) = rest.split_once("::")?;
        let doc = self.documents.iter().find(|d| d.path == path)?;
        Some(&doc.path)
    }
}

/// A provider document of the repository and what was read from it.
pub(crate) struct Document {
    pub(crate) path: String,
    pub(crate) summary: Summary,
}

/// A document read, as `meada export` shows it: path, format, version and its contracts,
/// sorted.
impl Serialize for Document {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Shown<'a> {
            path: &'a str,
            format: &'a Option<Format>,
            version: &'a Option<String>,
            contracts: BTreeSet<String>,
        }
        Shown {
            path: &self.path,
            format: &self.summary.format,
            version: &self.summary.version,
            contracts: (self.summary.routes.iter().map(|r| r.contract()))
                .chain(self.summary.channels.iter().map(|s| s.contract()))
                .collect(),
        }
        .serialize(s)
    }
}

fn documents_read<S: Serializer>(
    documents: &[Document],
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    s.collect_seq(documents.iter().filter(|d| d.summary.format.is_some()))
}

fn no_document_read(documents: &[Document]) -> bool {
    documents.iter().all(|d| d.summary.format.is_none())
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

/// The provider side of a contract: a symbol that serves it, a file, a repository, the call
/// that produces to a channel, or a route or channel side only a document lists
/// (`<repository>::<document>::<METHOD> <path>`, or `... <broker> send <name>`).
#[derive(Serialize, Default)]
pub(crate) struct Provider {
    pub(crate) contract: String,
    pub(crate) site: String,
    /// The path a gateway serves the route at, when `[serve]` rewrites it (`meada export`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) gateway: Option<String>,
    /// What the route's response or the channel's messages carry: from the first document by
    /// path that gives one, else from the code (`contracts.shape`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shape: Option<Shape>,
    /// Every path a gateway serves the route at, with the reason for the rewrite: the
    /// `[serve]` form first. `Http::declare` fills it; matching goes through
    /// `http::gateway_contracts`.
    #[serde(skip)]
    pub(crate) gateways: Vec<Gateway>,
    /// The hosts the repository serves on, lower case: `[serve] hosts` and the host of every
    /// `servers` URL of its documents. `Http::declare` fills it; `link` matches a URL's host
    /// against it.
    #[serde(skip)]
    pub(crate) hosts: Vec<String>,
    /// The `version` of a package's manifest, for `package::link`.
    #[serde(skip)]
    pub(crate) version: Option<String>,
}

/// A path a gateway serves a route at, and why: `gateway prefix /shop`.
#[derive(Clone, PartialEq)]
pub(crate) struct Gateway {
    pub(crate) path: String,
    pub(crate) reason: String,
}

/// A queue bound to an exchange by a pattern: the call `site` (a call site) that binds the
/// queue channel `contract`. A publish to the exchange reaches the queue when the pattern
/// matches its routing key.
#[derive(Serialize)]
pub(crate) struct Binding {
    pub(crate) contract: String,
    pub(crate) exchange: String,
    pub(crate) pattern: String,
    pub(crate) site: String,
}

/// A publish to the named exchange `exchange` with the routing key `key`, at the call `site`.
/// It is no channel's side by itself: it produces to every queue a binding of `exchange`
/// whose pattern matches `key` routes it to, and to no other (a publish to the empty exchange
/// is a plain producer of the queue its key names).
#[derive(Serialize)]
pub(crate) struct Publish {
    pub(crate) exchange: String,
    pub(crate) key: String,
    pub(crate) site: String,
}

/// The consumer side of a contract: a call site that uses it, or the symbol that reads a
/// resource or handles a channel; `contract` is `None` while an HTTP call is unresolved. `via` says
/// how it names the provider's repository, such as `host:api.example.com` or `env:API_URL`.
/// An unresolved one also keeps what could be read of its URL (see `Found`); the export leaves
/// that out.
#[derive(Serialize)]
pub(crate) struct Consumer {
    pub(crate) contract: Option<String>,
    pub(crate) site: String,
    pub(crate) via: Option<String>,
    /// What the messages a channel handler receives carry, from the first document by path
    /// that lists the side (`Channel::declare`); for an HTTP call, the fields it reads from
    /// the parsed response (`contracts.shape`, spec #23).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shape: Option<Shape>,
    #[serde(skip)]
    pub(crate) method: Option<String>,
    #[serde(skip)]
    pub(crate) fragments: Vec<String>,
    /// For a call through a package import, where it resolves in each provider of the package
    /// (`package::import`); `Packages::link` turns them into links.
    #[serde(skip)]
    pub(crate) hops: Vec<Hop>,
}

/// A `contracts.shape` value; one that does not parse reads as no shape.
fn stored_shape(json: Option<String>) -> Option<Shape> {
    // ponytail: written by `refresh` only, so a bad value is an index to rebuild.
    serde_json::from_str(&json?).ok()
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
            "SELECT contract, site, shape FROM {schema}.contracts WHERE role = 'provider' \
             ORDER BY contract, site"
        ))?
        .query_map([], |r| {
            Ok(Provider {
                contract: r.get(0)?,
                site: r.get(1)?,
                shape: stored_shape(r.get(2)?),
                ..Provider::default()
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let consumers = db
        .prepare(&format!(
            "SELECT contract, site, via, method, fragments, shape FROM {schema}.contracts \
             WHERE role = 'consumer' ORDER BY site, contract, via"
        ))?
        .query_map([], |r| {
            let fragments: Option<String> = r.get(4)?;
            Ok(Consumer {
                contract: r.get(0)?,
                site: r.get(1)?,
                via: r.get(2)?,
                shape: stored_shape(r.get(5)?),
                method: r.get(3)?,
                // ponytail: written by `refresh` only, so a bad value is an index to rebuild.
                fragments: fragments
                    .and_then(|f| serde_json::from_str(&f).ok())
                    .unwrap_or_default(),
                hops: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let bindings = db
        .prepare(&format!(
            "SELECT contract, via, method, site FROM {schema}.contracts \
             WHERE role = 'binding' ORDER BY contract, via, method, site"
        ))?
        .query_map([], |r| {
            Ok(Binding {
                contract: r.get(0)?,
                exchange: r.get(1)?,
                pattern: r.get(2)?,
                site: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let rows: Vec<(String, String)> = db
        .prepare(&format!(
            "SELECT path, summary FROM {schema}.documents ORDER BY path"
        ))?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let documents = (rows.into_iter())
        .map(|(path, summary)| {
            Ok(Document {
                path,
                summary: serde_json::from_str(&summary)?,
            })
        })
        .collect::<Result<_>>()?;
    let publishes = db
        .prepare(&format!(
            "SELECT via, method, site FROM {schema}.contracts \
             WHERE role = 'publish' ORDER BY via, method, site"
        ))?
        .query_map([], |r| {
            Ok(Publish {
                exchange: r.get(0)?,
                key: r.get(1)?,
                site: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let rows: Vec<(String, String)> = db
        .prepare(&format!(
            "SELECT path, summary FROM {schema}.manifests ORDER BY path"
        ))?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let (manifests, tsconfigs) = read_manifests(rows)?;
    // Only a repository with a manifest can import a package, and only one with a named
    // manifest provides one: the rows that resolve imports are read for those alone.
    let import_calls = match manifests.is_empty() {
        true => Vec::new(),
        false => db
            .prepare(&format!(
                "SELECT site, path, via, method FROM {schema}.contracts \
                 WHERE role = 'import' ORDER BY site, via, method"
            ))?
            .query_map([], |r| {
                Ok(ImportCall {
                    site: r.get(0)?,
                    path: r.get(1)?,
                    specifier: r.get(2)?,
                    chain: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?,
    };
    let exported = match manifests.iter().any(|m| m.summary.name.is_some()) {
        false => Vec::new(),
        true => db
            .prepare(&format!(
                "SELECT path, name, file, target FROM {schema}.exports \
                 ORDER BY path, name, file, target"
            ))?
            .query_map([], |r| {
                Ok(Exported {
                    path: r.get(0)?,
                    name: r.get(1)?,
                    file: r.get(2)?,
                    target: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?,
    };
    let shapes = match manifests.iter().any(|m| m.summary.name.is_some()) {
        false => Vec::new(),
        true => db
            .prepare(&format!(
                "SELECT path, method, via, shape FROM {schema}.contracts \
                 WHERE role = 'shape' ORDER BY path, method, via"
            ))?
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<(String, String, _, _)>>>()?
            .into_iter()
            .filter_map(|(path, name, want, shape)| {
                Some(package::ExportedShape {
                    path,
                    name,
                    want: Want::parse(want.as_deref()?)?,
                    shape: stored_shape(shape)?,
                })
            })
            .collect(),
    };
    let mut communities: Vec<Community> = Vec::new();
    for row in db
        .prepare(&format!(
            "SELECT name, member FROM {schema}.communities ORDER BY name, member"
        ))?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?
    {
        let (name, member) = row?;
        match communities.last_mut() {
            Some(c) if c.name == name => c.members.push(member),
            _ => communities.push(Community {
                name,
                members: vec![member],
            }),
        }
    }
    let entry_points = db
        .prepare(&format!(
            "SELECT identity, kind, label, site FROM {schema}.entry_points ORDER BY identity"
        ))?
        .query_map([], |r| {
            Ok((r.get(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<Vec<(String, String, String, String)>>>()?
        .into_iter()
        // ponytail: written by `refresh` only, so an unknown kind is an index to rebuild.
        .filter_map(|(identity, kind, label, site)| {
            let kind = EntryKind::parse(&kind)?;
            Some(EntryPoint {
                identity,
                kind,
                label,
                site,
            })
        })
        .collect();
    Ok(Export {
        repository: identity.to_string(),
        files,
        symbols,
        calls,
        providers,
        consumers,
        bindings,
        publishes,
        manifests,
        tsconfigs,
        packages: Vec::new(),
        dependencies: Vec::new(),
        import_calls,
        exported,
        shapes,
        imports: Vec::new(),
        communities,
        links: Vec::new(),
        entry_points,
        processes: Vec::new(),
        documents,
        stale_decisions: Vec::new(),
    })
}

/// The `manifests` rows, `(path, summary)`, as the `package.json` and `tsconfig.json` files they
/// hold.
fn read_manifests(rows: Vec<(String, String)>) -> Result<(Vec<Manifest>, Vec<Config>)> {
    let (mut manifests, mut tsconfigs) = (Vec::new(), Vec::new());
    for (path, summary) in rows {
        match package::is_tsconfig(&path) {
            // An invalid `tsconfig.json` is stored as `null` and counts as absent.
            true => tsconfigs.extend(
                serde_json::from_str::<Option<package::Tsconfig>>(&summary)?
                    .map(|tsconfig| Config { path, tsconfig }),
            ),
            false => manifests.push(Manifest {
                path,
                summary: serde_json::from_str(&summary)?,
            }),
        }
    }
    Ok((manifests, tsconfigs))
}

fn span(row: &rusqlite::Row, at: usize) -> rusqlite::Result<Span> {
    Ok(Span {
        path: row.get(at)?,
        start: row.get(at + 1)?,
        end: row.get(at + 2)?,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{SCHEMA, SCHEMA_VERSION, refresh_stamped};
    use crate::{document, lang, package};

    #[test]
    fn a_stamped_file_that_vanished_or_cannot_be_read_has_no_row() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.execute(
            "INSERT INTO manifests VALUES ('gone/package.json', 'h', 1, 1, '{}')",
            [],
        )
        .unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let found = vec![
            (
                "gone/package.json".to_string(),
                root.join("gone/package.json"),
            ),
            // A directory stats fine and cannot be read as a file.
            ("src".to_string(), root.join("src")),
            ("Cargo.toml".to_string(), root.join("Cargo.toml")),
        ];
        let tx = db.transaction().unwrap();

        refresh_stamped(
            &tx,
            "manifests",
            &found,
            std::time::SystemTime::now(),
            |_, _| Ok("{}".into()),
        )
        .unwrap();

        let paths: Vec<String> = (tx.prepare("SELECT path FROM manifests").unwrap())
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(paths, ["Cargo.toml"]);
    }

    /// What an index of this version stores: `SCHEMA`, then the facts each pack stores for its
    /// sample in `tests/fixtures/schema/`, then the summary of each provider document sample. Its golden is keyed by `SCHEMA_VERSION` and never
    /// rewritten in place, so a change fails until the version is bumped; `MEADA_BLESS=1` then
    /// writes the new version's golden and removes the old one.
    #[test]
    fn the_stored_format_changes_only_with_the_schema_version() {
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut actual = SCHEMA.trim_start().to_string();
        for pack in lang::packs() {
            let name = format!("sample.{}", pack.extensions()[0]);
            let src = fs::read(tests.join("fixtures/schema").join(&name)).unwrap_or_else(|_| {
                panic!("add tests/fixtures/schema/{name}, a sample of the facts its pack stores")
            });
            let facts = pack.parse(&name, &src).unwrap();
            let facts = serde_json::to_string_pretty(&facts).unwrap();
            actual += &format!("\n{name}\n{facts}\n");
        }
        for name in ["sample.openapi.yaml", "sample.asyncapi.yaml"] {
            let src = fs::read(tests.join("fixtures/schema").join(name)).unwrap();
            let summary = serde_json::to_string_pretty(&document::read(name, &src)).unwrap();
            actual += &format!("\n{name}\n{summary}\n");
        }
        let src = fs::read(tests.join("fixtures/schema/sample.package.json")).unwrap();
        let summary = serde_json::to_string_pretty(&package::read(&src)).unwrap();
        actual += &format!("\nsample.package.json\n{summary}\n");
        let src = fs::read(tests.join("fixtures/schema/sample.tsconfig.json")).unwrap();
        let summary = serde_json::to_string_pretty(&package::read_tsconfig(&src)).unwrap();
        actual += &format!("\nsample.tsconfig.json\n{summary}\n");

        let golden = tests.join(format!("golden/schema.v{SCHEMA_VERSION}.txt"));
        let Ok(expected) = fs::read_to_string(&golden) else {
            assert!(
                std::env::var_os("MEADA_BLESS").is_some_and(|v| v == "1"),
                "missing {}; run with MEADA_BLESS=1 to create it",
                golden.display()
            );
            for old in fs::read_dir(tests.join("golden")).unwrap() {
                let old = old.unwrap().file_name().into_string().unwrap();
                if old.starts_with("schema.v") && old.ends_with(".txt") {
                    fs::remove_file(tests.join("golden").join(old)).unwrap();
                }
            }
            fs::write(&golden, actual).unwrap();
            return;
        };
        assert!(
            actual == expected,
            "the stored format differs from {}: bump SCHEMA_VERSION, then rerun with \
             MEADA_BLESS=1 to write the new version's golden\n--- actual ---\n{actual}",
            golden.display()
        );
    }
}
