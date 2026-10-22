//! The index: one SQLite file per repository, holding spans and identities but no source text
//! (ADR 0004). Locally it lives in the checkout's own `.meada/`; in server mode, at
//! `<index_dir>/<identity>/index.db` (ADR 0011). `workspace::Repository::index` says which.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use rusqlite::config::DbConfig;
use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, params};
use serde::{Serialize, Serializer};

use crate::Result;
use crate::community::{self, Community};
use crate::config::Serve;
use crate::contract::{self, Level, Link, Names, Role, Site};
use crate::decisions::Stale;
use crate::document::{self, Format, Summary};
use crate::entry::{self, EntryKind, EntryPoint};
use crate::lang::{self, Facts};
use crate::msg::{Fix, Msg};
use crate::package::{self, Config, Dependency, Hop, Import, ImportCall, Manifest, Package};
use crate::resolve::{Code, Exported, Resolution, Resolver, Target};
use crate::shape::{Shape, Want};
use crate::trace::Process;

/// The stored format's version, derived from its golden (`tests/golden/schema.txt`, kept by the
/// unit test `the_stored_format_matches_its_golden`): re-blessing it after a change to `SCHEMA`,
/// to the facts a pack stores or to a document's or a manifest's stored `summary` changes the
/// version, and an index with another version is rebuilt beside it and renamed over it.
pub(crate) fn schema_version() -> i32 {
    let hash = blake3::hash(include_bytes!("../tests/golden/schema.txt"));
    let [a, b, c, d, ..] = *hash.as_bytes();
    // A new SQLite file starts at `user_version` 0, which an odd version never equals.
    i32::from_le_bytes([a, b, c, d]) | 1
}

/// `meta` holds the repository identity and the BLAKE3 of the packs' config files.
/// `files.mtime` (nanoseconds) and `size` are the stat seen when the file was hashed; `mtime`
/// is NULL when it was too recent to trust. `files.facts` is what the pack parsed, as JSON
/// (names, specifiers and byte ranges, no source text); `files.resolved` is the BLAKE3 of the
/// file's resolved symbol and call rows, empty for a file its pack leaves out (`Layout::members`: a
/// Rust file no `mod` reaches), which has no rows and is no file of the export. `calls.target` holds the one definition an `exact` or
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
/// is cached too. `manifests` holds each `package.json` (spec #22), `pyproject.toml` and
/// `requirements*.txt` (spec #24) the same way, its `summary` being `package::Summary` as JSON
/// (name, version, dependency entries, the entry fields `exports`, `types`, `module` and `main`,
/// and a `pyproject.toml`'s source `roots`), read from the bytes alone; a manifest without a
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
    facts TEXT NOT NULL, resolved TEXT NOT NULL, setting TEXT NOT NULL
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

/// Where the index of the checkout at `root` lives locally, inside its own `.meada/`.
pub fn path(root: &Path) -> PathBuf {
    root.join(".meada/index.db")
}

/// What `status` reports about one repository's index, read without writing.
pub enum State {
    Missing,
    OtherSchema,
    /// A commit in progress kept it locked past the busy timeout.
    Locked,
    Indexed {
        files: i64,
    },
}

/// The state of the index at `path`, read as `open_read` reads.
pub fn state(path: &Path) -> State {
    let failed = |e: rusqlite::Error| match e.sqlite_error_code() {
        Some(ErrorCode::CannotOpen) => State::Missing,
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => State::Locked,
        _ => State::OtherSchema,
    };
    let db = match read_only(path) {
        Ok(db) => db,
        Err(e) => return failed(e),
    };
    match db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0)) {
        Ok(version) if version == schema_version() => {}
        Ok(_) => return State::OtherSchema,
        Err(e) => return failed(e),
    }
    match db.query_row("SELECT count(*) FROM files WHERE resolved != ''", [], |r| {
        r.get(0)
    }) {
        Ok(files) => State::Indexed { files },
        Err(e) => failed(e),
    }
}

/// How long a read waits for a commit in progress on an index before it fails (ADR 0011).
const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

thread_local! {
    static BUSY: std::cell::Cell<Duration> = const { std::cell::Cell::new(BUSY_TIMEOUT) };
}

/// Runs `f` with this thread's read-only opens waiting at most `timeout` for a commit in
/// progress, instead of 10 seconds. No option or setting reaches it: tests, and the readiness
/// probe of `status --workspace`.
#[doc(hidden)]
pub fn with_busy_timeout<T>(timeout: Duration, f: impl FnOnce() -> T) -> T {
    struct Restore(Duration);
    impl Drop for Restore {
        fn drop(&mut self) {
            BUSY.set(self.0);
        }
    }
    let _restore = Restore(BUSY.replace(timeout));
    f()
}

/// The index at `path` opened read-only: nothing is created, and a statement that meets a
/// commit in progress waits for it up to the busy timeout. The file may come from a store
/// (ADR 0012), so its schema is trusted with nothing: `trusted_schema` is off (no view or
/// trigger calls a function that is not innocuous) and the connection is defensive (no
/// statement corrupts the file, nor writes the schema through a pragma).
fn read_only(path: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    db.pragma_update(None, "trusted_schema", false)?;
    db.busy_timeout(BUSY.get())?;
    Ok(db)
}

/// The schema objects of `db`'s `main`: type, name, table and SQL, sorted.
#[cfg(feature = "snapshot")]
fn objects(db: &Connection) -> rusqlite::Result<Vec<[Option<String>; 4]>> {
    let mut statement =
        db.prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY 1, 2, 3, 4")?;
    let rows = statement.query_map([], |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?]))?;
    rows.collect()
}

/// The index at `index`, of the repository `repo` in a pulled generation, holds exactly the
/// schema objects this build creates (`SCHEMA`): no view, trigger, table or index more, and
/// none altered or missing, compared by type, name, table and SQL with a fresh index. A
/// crafted `symbols` view with an endless recursive query would hang every call. Before it is
/// opened, its header must name the rollback journal (bytes 18 and 19 both 1): a read-only open
/// of a WAL index would create `-wal` and `-shm` files in the generation. Checked by `meada
/// snapshot pull` and the start check of `--generations`; the error names the repository and
/// the object.
#[cfg(feature = "snapshot")]
pub fn vet(index: &Path, repo: &str, fix: Fix) -> Result<()> {
    let mut header = [0; 20];
    let read = fs::File::open(index).and_then(|mut f| f.read_exact(&mut header));
    if read.is_err() || header[18..20] != [1, 1] {
        return Err(Msg::IndexNotRollback(repo).text().into());
    }
    let db = read_only(index).map_err(|e| unreadable(e, index, repo, fix))?;
    let found = objects(&db).map_err(|e| unreadable(e, index, repo, fix))?;
    let fresh = Connection::open_in_memory()?;
    fresh.execute_batch(SCHEMA)?;
    let expected = objects(&fresh)?;
    let odd = (found.iter().find(|o| !expected.contains(o)))
        .or_else(|| expected.iter().find(|o| !found.contains(o)));
    match odd {
        None => Ok(()),
        Some([kind, name, ..]) => Err(Msg::IndexSchemaObject {
            repo,
            kind: kind.as_deref().unwrap_or_default(),
            name: name.as_deref().unwrap_or_default(),
        }
        .text()
        .into()),
    }
}

/// Opens the indexes of `repos` (index path, repository identity) read-only, as server mode
/// reads (ADR 0011): the first as `main`, each other attached as `schema(n)`. Nothing is
/// created, rebuilt or written. A missing index, one of another schema, one a commit left
/// interrupted, or one a commit in progress keeps locked past the busy timeout is an error
/// naming its repository, all but the last with its fix (`Fix`).
pub fn open_read(repos: &[(&Path, &str)], fix: Fix) -> Result<Connection> {
    let [(index, repo), ..] = repos else {
        return Err("no repository to open".into());
    };
    let db = read_only(index).map_err(|e| unreadable(e, index, repo, fix))?;
    for (n, (index, repo)) in repos.iter().enumerate().skip(1) {
        let path = index.to_str().ok_or("index path is not UTF-8")?;
        (db.execute(&format!("ATTACH DATABASE ?1 AS {}", schema(n)), [path]))
            .map_err(|e| unreadable(e, index, repo, fix))?;
    }
    // One read transaction, held until the connection closes (and then rolled back, nothing
    // being written): its first read of each index takes the lock a commit waits on, so every
    // answer read through it sees each index as one commit left it (ADR 0011).
    db.execute_batch("BEGIN DEFERRED")?;
    for (n, (index, repo)) in repos.iter().enumerate() {
        check_read(&db, &schema(n), index, repo, fix)?;
    }
    Ok(db)
}

/// The schema the index of the `n`th repository of a workspace is read as: `main`, `peer<n>`.
pub fn schema(n: usize) -> String {
    if n == 0 {
        "main".into()
    } else {
        format!("peer{n}")
    }
}

/// The schema version check of a read-only open; the first read, so the one that waits.
fn check_read(db: &Connection, schema: &str, index: &Path, repo: &str, fix: Fix) -> Result<()> {
    let version = (db.query_row(&format!("PRAGMA {schema}.user_version"), [], |r| {
        r.get::<_, i32>(0)
    }))
    .map_err(|e| unreadable(e, index, repo, fix))?;
    if version != schema_version() {
        return Err(Msg::IndexOtherVersion { repo, fix }.text().into());
    }
    Ok(())
}

/// A read-only open's SQLite error as `open_read` names it.
fn unreadable(
    e: rusqlite::Error,
    index: &Path,
    repo: &str,
    fix: Fix,
) -> Box<dyn std::error::Error> {
    let text = match e.sqlite_error_code() {
        Some(ErrorCode::CannotOpen) => Msg::IndexMissing { repo, fix }.text(),
        Some(ErrorCode::NotADatabase) => Msg::IndexOtherVersion { repo, fix }.text(),
        // A hot journal (a writer stopped mid-commit) that only a write can roll back.
        Some(ErrorCode::ReadOnly) => Msg::IndexInterrupted { repo, fix }.text(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Msg::IndexLocked {
            repo,
            index: &index.display().to_string(),
            timeout: BUSY.get(),
        }
        .text(),
        _ => return e.into(),
    };
    text.into()
}

/// Opens the index at `index` of the checkout at `root` (repository `identity`, documents per
/// `serve`), creating its directory. A missing index, or one of another schema, is rebuilt
/// beside it and renamed over it (`rebuild`), so a reader sees the old index or the whole new
/// one. The checkout's own `.meada/` gets a `.gitignore` of `*`; an index elsewhere (server
/// mode's `index_dir`) is outside any checkout and gets none.
pub fn open(index: &Path, root: &Path, identity: &str, serve: &Serve) -> Result<Connection> {
    let dir = index.parent().ok_or("index path has no directory")?;
    fs::create_dir_all(dir)?;
    if index == path(root) {
        fs::write(dir.join(".gitignore"), "*\n")?;
    }
    match current(index)? {
        Some(db) => Ok(db),
        None => rebuild(index, root, identity, serve),
    }
}

/// The index at `index` opened to write, when it has the current schema. Its first read rolls
/// back the hot journal a writer stopped mid-commit left. An unreadable file counts as a
/// mismatch (the index's directory is Meada's own); a missing one is not created, so no reader
/// finds an empty file; a commit in progress past the busy timeout is an error, never a reason
/// to rebuild.
fn current(index: &Path) -> Result<Option<Connection>> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let db = match Connection::open_with_flags(index, flags) {
        Err(e) if e.sqlite_error_code() == Some(ErrorCode::CannotOpen) => return Ok(None),
        db => db?,
    };
    db.busy_timeout(BUSY.get())?;
    match db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0)) {
        Ok(version) => Ok((version == schema_version()).then_some(db)),
        Err(e)
            if matches!(
                e.sqlite_error_code(),
                Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
            ) =>
        {
            Err(e.into())
        }
        Err(_) => Ok(None),
    }
}

/// `index` with `suffix` appended to its file name.
fn beside(index: &Path, suffix: &str) -> PathBuf {
    let mut name = index.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// Rebuilds the index at `index` without ever removing it (spec #259): under an exclusive
/// lock on `index.db.lock` (created once, never removed), a whole new index is built and
/// committed in `index.db.new.<pid>`, closed, and renamed over `index.db`. A reader that holds
/// the old file keeps answering from it. A second rebuild waits for the lock, then finds the
/// index rebuilt; the `index.db.new.*` a crashed rebuild left are removed only under the lock,
/// so none belongs to a live rebuild.
fn rebuild(index: &Path, root: &Path, identity: &str, serve: &Serve) -> Result<Connection> {
    let path = beside(index, ".lock");
    let locked = (fs::File::options().read(true).write(true).create(true))
        .truncate(false)
        .open(&path)
        .and_then(|lock| lock.lock().map(|()| lock));
    let _lock = locked.map_err(|e| {
        let lock = path.display().to_string();
        Msg::RebuildLock {
            lock: &lock,
            error: &e.to_string(),
        }
        .text()
    })?;
    if let Some(db) = current(index)? {
        return Ok(db);
    }
    let dir = index.parent().ok_or("index path has no directory")?;
    let name = index
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("index path is not UTF-8")?;
    let crashed = format!("{name}.new.");
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with(&crashed))
        {
            fs::remove_file(entry.path())?;
        }
    }
    // `current` opened the old index to write, rolling back any hot journal it could; one left
    // is not that index's (SQLite would take it for the new one's).
    match fs::remove_file(beside(index, "-journal")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let new = beside(index, &format!(".new.{}", std::process::id()));
    let built = (|| -> Result<()> {
        // Closed before the rename, its last commit having left no journal.
        let db = writable(&new)?;
        db.execute_batch(SCHEMA)?;
        db.pragma_update(None, "user_version", schema_version())?;
        refresh(&db, root, identity, serve)
    })();
    // Where a reader holding the old file forbids the rename (Windows), it stays as it was.
    let replaced = built.and_then(|()| {
        fs::rename(&new, index).map_err(|e| {
            let index = index.display().to_string();
            Msg::IndexNotReplaced {
                index: &index,
                error: &e.to_string(),
            }
            .text()
            .into()
        })
    });
    if let Err(e) = replaced {
        let _ = fs::remove_file(&new);
        return Err(e);
    }
    Ok(writable(index)?)
}

/// The index at `path` opened to write. Its commits, and its first read, wait up to the busy
/// timeout for the readers and the commit in progress of other connections (a server's reads,
/// ADR 0011), rather than failing at once or taking a locked index for one of another schema.
fn writable(path: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open(path)?;
    db.busy_timeout(BUSY.get())?;
    Ok(db)
}

/// What the index knows of a file.
struct Stored {
    hash: String,
    mtime: Option<i64>,
    size: i64,
    resolved: String,
    /// `files.setting`: `Facts::setting` of the stored facts.
    setting: String,
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

/// What `refresh` read of the repository apart from the pack's facts, for
/// `ContractKind::extract`, built once per refresh before the fixpoint: a kind whose contracts
/// depend on what the manifests say reads it.
pub struct Indexed<'a> {
    /// The manifests, as `read_manifests` returns them (the Cargo inheritance applied).
    pub manifests: &'a [Manifest],
    /// The `tsconfig.json` files read, as the manifest reading step returns them.
    pub tsconfigs: &'a [Config],
    /// What the files of every pack export (the `exports` table).
    pub exported: &'a [Exported],
    /// The files of every pack, as kept.
    pub files: &'a [&'a str],
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
/// that do not parse (a change whose golden was not re-blessed) rebuild the index.
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
    let stored = stored_files(&tx)?;

    let all = walk(root);
    refresh_stamped(
        &tx,
        "documents",
        &documents_found(root, &all, serve),
        now,
        |abs| read_up_to(abs, document::MAX_BYTES),
        |path, src| Ok(serde_json::to_string(&document::read(path, src))?),
    )?;
    refresh_stamped(
        &tx,
        "manifests",
        &manifests_found(&all),
        now,
        |abs| read_up_to(abs, lang::MAX_BYTES),
        |path, src| match package::is_tsconfig(path) {
            true => Ok(serde_json::to_string(&package::read_tsconfig(src))?),
            false => Ok(serde_json::to_string(&package::read_manifest(path, src))?),
        },
    )?;
    // The packages decide which files keep the shapes of what they export (issue #175).
    let manifest_rows = manifest_rows(&tx)?;
    let Packs {
        walked,
        packs,
        configs,
        digest,
        settings,
    } = packs_of(root, &all);
    let mut changed = Vec::new();
    for &(path, abs, pack) in &walked {
        let setting = settings[&pack](path);
        let (size, mtime, trusted) = stat(abs, now)?;
        let known = stored.get(path);
        let current = known.is_some_and(|k| k.setting == setting);
        if current && known.is_some_and(|k| k.mtime.is_some() && k.mtime == mtime && k.size == size)
        {
            continue;
        }
        let mtime = mtime.filter(|_| trusted);
        let src = fs::read(abs)?;
        let hash = blake3::hash(&src).to_hex().to_string();
        if current && known.is_some_and(|k| k.hash == hash) {
            tx.execute(
                "UPDATE files SET mtime = ?2, size = ?3 WHERE path = ?1",
                params![path, mtime, size],
            )?;
            continue;
        }
        let facts = lang::pack(pack).parse_with(path, &src, &setting)?;
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

    // An edited `package.json` or `tsconfig.json` changes which files keep shapes (`surface`),
    // so it extracts again. Folding them in also forces that on an index of this version written
    // before the `shape` rows and constant `exports` targets existed (issue #175), which the
    // schema golden does not cover.
    let digest = configs_digest(digest, &manifest_rows);
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
        // Facts this version cannot read, stored before a change whose golden was not
        // re-blessed: rebuild, as for another version. Without the identity, the next pass starts
        // over and keeps nothing.
        drop(tx);
        db.execute("DELETE FROM meta", [])?;
        return refresh(db, root, identity, serve);
    };
    let parsed: BTreeMap<&str, &Facts> = changed
        .iter()
        .map(|p| (p.path.as_str(), &p.facts))
        .chain(kept.iter().map(|(path, facts)| (path.as_str(), facts)))
        .collect();

    // Calls resolve among the files of their own pack, those the pack builds.
    let mut groups: BTreeMap<usize, BTreeMap<&str, &Facts>> = BTreeMap::new();
    for (&path, &facts) in &parsed {
        groups.entry(packs[path]).or_default().insert(path, facts);
    }
    let found_configs = |pack: usize| -> Vec<(&str, &[u8])> {
        (configs[&pack].iter())
            .map(|(n, c)| (*n, c.as_slice()))
            .collect()
    };
    // What each pack makes of the whole repository, once (`LanguagePack::layout`).
    let mut layouts = BTreeMap::new();
    for (&pack, files) in &mut groups {
        let layout = lang::pack(pack).layout(&found_configs(pack), files)?;
        if let Some(keep) = &layout.members {
            files.retain(|path, _| keep.contains(*path));
        }
        layouts.insert(pack, layout);
    }
    let all: BTreeMap<&str, &Facts> = groups.values().flatten().map(|(p, f)| (*p, *f)).collect();
    // A file its pack leaves out keeps its facts, for when a change brings it in, but no rows:
    // an empty `resolved` both marks it and makes the rows be written again then.
    for path in parsed.keys().filter(|p| !all.contains_key(*p)) {
        if stored.get(*path).is_some_and(|s| s.resolved.is_empty()) {
            continue;
        }
        forget_calls(&tx, path)?;
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        tx.execute(
            "UPDATE files SET resolved = '' WHERE path = ?1",
            params![path],
        )?;
    }
    let mut roots: Vec<(String, String)> = Vec::new();
    let mut resolvers: BTreeMap<usize, Resolver> = BTreeMap::new();
    for (&pack, files) in &groups {
        let layout = layouts.remove(&pack).expect("a layout per pack");
        roots.extend(layout.programs);
        let resolver =
            Resolver::with_shared(files, layout.modules, layout.shared, lang::pack(pack));
        let resolver = resolver.publishing(layout.published);
        resolvers.insert(pack, resolver.reaching(layout.reaches));
    }
    let (manifests, tsconfigs) = read_manifests(manifest_rows)?;
    let files: Vec<&str> = all.keys().copied().collect();
    let exported: Vec<Exported> = resolvers.values().flat_map(Resolver::exported).collect();
    let indexed = Indexed {
        manifests: &manifests,
        tsconfigs: &tsconfigs,
        exported: &exported,
        files: &files,
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
                found.extend(kind.extract(&code, &indexed));
            }
            misses.extend(code.misses().into_iter().map(String::from));
        }
        if misses.is_empty() {
            break found;
        }
        for path in misses {
            let src = fs::read(root.join(&path))?;
            let pack = packs[path.as_str()];
            let facts = lang::pack(pack).parse_with(&path, &src, &settings[&pack](&path))?;
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
    // The programs the packs declare per file, and the console scripts of the manifests.
    let scripts = package::programs(&manifests, &files);
    let programs: Vec<entry::Program> = (all.iter())
        .flat_map(|(&path, &facts)| {
            (facts.programs.iter()).map(move |def| (path.to_string(), def.clone()))
        })
        .chain(scripts)
        .chain(roots)
        .map(|(file, def)| entry::Program { file, def })
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

/// The `files` rows, by path.
fn stored_files(db: &Connection) -> Result<HashMap<String, Stored>> {
    let rows = db
        .prepare("SELECT path, hash, mtime, size, resolved, setting FROM files")?
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                Stored {
                    hash: r.get(1)?,
                    mtime: r.get(2)?,
                    size: r.get(3)?,
                    resolved: r.get(4)?,
                    setting: r.get(5)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The `manifests` rows (path and summary), by path.
fn manifest_rows(db: &Connection) -> Result<Vec<(String, String)>> {
    let rows = db
        .prepare("SELECT path, summary FROM manifests ORDER BY path")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// The files of a walk a language pack handles, and what the packs' config files make of them.
struct Packs<'a> {
    /// Each file a pack handles: path, absolute path and pack.
    walked: Vec<(&'a String, &'a PathBuf, usize)>,
    /// The pack of each of them, by path.
    packs: BTreeMap<&'a str, usize>,
    /// The config files read, per pack: path and content.
    configs: BTreeMap<usize, Vec<(&'a str, Vec<u8>)>>,
    /// The BLAKE3 of the config files read, before `configs_digest` folds the manifests in.
    digest: blake3::Hasher,
    /// What the configs make of each file's facts (`LanguagePack::settings`), per pack: a file
    /// parsed under another setting is parsed again, whatever its content.
    settings: BTreeMap<usize, Setting>,
}

/// What `LanguagePack::settings` makes of a file's facts, by path.
type Setting = Box<dyn Fn(&str) -> String>;

/// The packs of the files `walk` found under `root`, their config files read (`read_config`)
/// on every call: they are few and small. A `**/` name takes every file of that name the walk
/// found, in path order.
fn packs_of<'a>(root: &Path, all: &'a [(String, PathBuf)]) -> Packs<'a> {
    let walked: Vec<(&String, &PathBuf, usize)> = (all.iter())
        .filter_map(|(path, abs)| Some((path, abs, lang::pack_for(path)?)))
        .collect();
    let packs: BTreeMap<&str, usize> = walked.iter().map(|(p, _, k)| (p.as_str(), *k)).collect();
    let mut configs: BTreeMap<usize, Vec<(&str, Vec<u8>)>> =
        packs.values().map(|&pack| (pack, Vec::new())).collect();
    let mut digest = blake3::Hasher::new();
    for (&pack, found) in &mut configs {
        for &name in lang::pack(pack).config_files() {
            let paths: BTreeSet<&str> = match name.strip_prefix("**/") {
                Some(file) => (all.iter())
                    .map(|(path, _)| path.as_str())
                    .filter(|path| path.rsplit('/').next() == Some(file))
                    .collect(),
                None => BTreeSet::from([name]),
            };
            for path in paths {
                if let Some(src) = read_config(root, path) {
                    digest.update(format!("{pack}\0{path}\0{}\0", src.len()).as_bytes());
                    digest.update(&src);
                    found.push((path, src));
                }
            }
        }
    }
    let settings = (configs.iter())
        .map(|(&pack, found)| {
            let found: Vec<(&str, &[u8])> = found.iter().map(|(n, c)| (*n, c.as_slice())).collect();
            (pack, lang::pack(pack).settings(&found))
        })
        .collect();
    Packs {
        walked,
        packs,
        configs,
        digest,
        settings,
    }
}

/// `meta.configs`: the config files' digest with the stored manifests folded in.
fn configs_digest(mut digest: blake3::Hasher, manifest_rows: &[(String, String)]) -> String {
    for (path, summary) in manifest_rows {
        digest.update(format!("{path}\0{summary}\0").as_bytes());
    }
    digest.finalize().to_hex().to_string()
}

/// Whether `refresh` would find nothing to change in the index `db` holds for the checkout at
/// `root` (repository `identity`, documents per `serve`), checked as it checks, reading only:
/// the same walk, then each stamped file's stat, and its hash when the stat moved. A file whose
/// stat moved but whose content did not (one a refresh would only stamp again, such as a file
/// indexed within `RACY` of its last write) is no difference. `meada snapshot publish`
/// (ADR 0012) runs it on an index opened with `open_read`.
#[cfg(feature = "snapshot")]
pub fn fresh(db: &Connection, root: &Path, identity: &str, serve: &Serve) -> Result<bool> {
    let now = SystemTime::now();
    let meta = (db.query_row("SELECT identity, configs FROM meta", [], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }))
    .optional()?;
    let Some((_, configs)) = meta.filter(|(known, _)| known == identity) else {
        return Ok(false);
    };
    // The same stamp a refresh trusts: a trusted mtime and the size, or else the hash.
    let same = |hash: &str,
                mtime: Option<i64>,
                size: i64,
                abs: &Path,
                read: &dyn Fn(&Path) -> std::io::Result<Vec<u8>>| {
        let Ok((now_size, now_mtime, _)) = stat(abs, now) else {
            return false;
        };
        if mtime.is_some() && mtime == now_mtime && size == now_size {
            return true;
        }
        read(abs).is_ok_and(|src| blake3::hash(&src).to_hex().as_str() == hash)
    };
    let all = walk(root);
    let tables = [
        (
            "documents",
            documents_found(root, &all, serve),
            document::MAX_BYTES,
        ),
        ("manifests", manifests_found(&all), lang::MAX_BYTES),
    ];
    for (table, found, max) in tables {
        let read = |abs: &Path| read_up_to(abs, max);
        let stored: HashMap<String, Stamp> = db
            .prepare(&format!("SELECT path, hash, mtime, size FROM {table}"))?
            .query_map([], |r| {
                let (hash, mtime, size) = (r.get(1)?, r.get(2)?, r.get(3)?);
                Ok((r.get(0)?, Stamp { hash, mtime, size }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut seen = 0;
        for (path, abs) in &found {
            match stored.get(path) {
                Some(k) if same(&k.hash, k.mtime, k.size, abs, &read) => seen += 1,
                Some(_) => return Ok(false),
                // A file a refresh cannot stat or read gets no row either.
                None if stat(abs, now).is_err() || read(abs).is_err() => {}
                None => return Ok(false),
            }
        }
        if seen != stored.len() {
            return Ok(false);
        }
    }
    let stored = stored_files(db)?;
    let Packs {
        walked,
        packs,
        digest,
        settings,
        ..
    } = packs_of(root, &all);
    let read = |abs: &Path| fs::read(abs);
    for &(path, abs, pack) in &walked {
        let Some(k) = stored.get(path) else {
            return Ok(false);
        };
        if k.setting != settings[&pack](path) || !same(&k.hash, k.mtime, k.size, abs, &read) {
            return Ok(false);
        }
    }
    if stored.keys().any(|p| !packs.contains_key(p.as_str())) {
        return Ok(false);
    }
    Ok(configs_digest(digest, &manifest_rows(db)?) == configs)
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

/// The pack config file at `path` under `root` (`tsconfig.json`, `pyproject.toml`, each
/// `Cargo.toml`), read as a manifest is (`read_up_to`): the pack's reader takes one past
/// `lang::MAX_BYTES` as invalid. Like the walk, the read goes through no symbolic link.
pub(crate) fn read_config(root: &Path, path: &str) -> Option<Vec<u8>> {
    read_up_to(&plain_file(root, path)?, lang::MAX_BYTES).ok()
}

/// A file read up to one byte past `max`, the size its reader takes as invalid past, whatever
/// the rest holds (`lang::MAX_BYTES` for a manifest or pack config, `document::MAX_BYTES`
/// for a provider document): the rest is never read, and what was read stands for the whole
/// file wherever it is hashed.
fn read_up_to(abs: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    let mut src = Vec::new();
    fs::File::open(abs)?
        .take(max as u64 + 1)
        .read_to_end(&mut src)?;
    Ok(src)
}

/// The manifests of a repository (`package.json`, `pyproject.toml`, `requirements*.txt`,
/// `Cargo.toml`) and its
/// `tsconfig.json` files (path and absolute path), but those under a `node_modules` directory,
/// which the walk finds when `.gitignore` does not list it.
fn manifests_found(walked: &[(String, PathBuf)]) -> Vec<(String, PathBuf)> {
    let is_manifest =
        |path: &str| package::is_manifest(path) && !path.split('/').any(|s| s == "node_modules");
    (walked.iter())
        .filter(|(path, _)| is_manifest(path))
        .cloned()
        .collect()
}

/// Brings `table` (`documents` or `manifests`, both stamped like `files`) up to date with
/// `found`, the files on disk (path and absolute path), as `refresh` does `files`: stat, then
/// `read` and hash, then `summarize` only what changed. A stored row not found, or whose file
/// cannot be statted or read, leaves the table.
fn refresh_stamped(
    tx: &Transaction,
    table: &str,
    found: &[(String, PathBuf)],
    now: SystemTime,
    read: impl Fn(&Path) -> std::io::Result<Vec<u8>>,
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
        let Ok(src) = read(abs) else {
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
        "INSERT OR REPLACE INTO files VALUES (?1, ?2, ?3, ?4, ?5, '', ?6)",
        params![
            p.path,
            p.hash,
            p.mtime,
            p.size,
            serde_json::to_string(&p.facts)?,
            p.facts.setting
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
    /// Where the repository stands in the workspace; set by the engine, never stored or shown.
    #[serde(skip)]
    pub(crate) root: PathBuf,
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
    /// The repository by identity and root, as `Ecosystem::reaches` takes it.
    pub(crate) fn place(&self) -> (&str, &Path) {
        (&self.repository, &self.root)
    }

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
            "SELECT path, hash FROM {schema}.files WHERE resolved != '' ORDER BY path"
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
        root: PathBuf::new(),
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
    // A Cargo member's `workspace = true` entries read from a manifest above it.
    package::inherit(&mut manifests);
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
    use std::time::{Duration, Instant};

    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use rusqlite::{Connection, OpenFlags};

    use super::{
        SCHEMA, State, open, open_read, read_config, refresh_stamped, state, with_busy_timeout,
    };
    use crate::config::Serve;
    use crate::msg::Fix;
    use crate::{document, lang, package};

    const REPO: &str = "example.com/acme/web";
    const MANIFEST: &str = "ops/meada.workspace.toml";

    /// An empty directory for `case`, and where an index in it lives.
    fn index_dir(case: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("meada-index-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let index = dir.join(REPO).join("index.db");
        (dir, index)
    }

    /// The index of `REPO` at `index`, opened as server mode reads it.
    fn read(index: &Path) -> crate::Result<Connection> {
        open_read(&[(index, REPO)], Fix::Index(MANIFEST))
    }

    fn communities(db: &Connection) -> i64 {
        (db.query_row("SELECT count(*) FROM communities", [], |r| r.get(0))).unwrap()
    }

    /// Server mode (ADR 0011): a read that meets a commit in progress waits for it, and past
    /// the busy timeout fails naming the repository and the lock.
    #[test]
    fn a_read_only_open_waits_for_a_commit_in_progress_up_to_the_busy_timeout() {
        let (dir, index) = index_dir("busy");
        let writer = open(&index, &dir, REPO, &Serve::default()).unwrap();
        // The lock a commit takes: no reader gets in until it ends.
        (writer.execute_batch("BEGIN EXCLUSIVE; INSERT INTO communities VALUES ('c', 'm');"))
            .unwrap();

        let held = with_busy_timeout(Duration::from_millis(50), || read(&index));
        let expected = format!(
            "example.com/acme/web: the index `{}` stayed locked by a commit in progress for \
             longer than 50ms",
            index.display()
        );
        assert_eq!(held.err().map(|e| e.to_string()), Some(expected));
        let locked = with_busy_timeout(Duration::from_millis(50), || state(&index));
        assert!(matches!(locked, State::Locked));

        let commit = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            writer.execute_batch("COMMIT").unwrap();
        });
        let start = Instant::now();
        let db = read(&index).unwrap();
        assert!(start.elapsed() >= Duration::from_millis(150));
        assert_eq!(communities(&db), 1);
        commit.join().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    /// One answer reads every index as one commit left it: a commit that lands meanwhile waits
    /// for the answer to end, then lands.
    #[test]
    fn a_read_only_open_sees_one_commit_until_it_closes_and_a_commit_waits_for_it() {
        let (dir, index) = index_dir("snapshot");
        let writer = open(&index, &dir, REPO, &Serve::default()).unwrap();
        let db = read(&index).unwrap();
        assert_eq!(communities(&db), 0);

        let commit = std::thread::spawn(move || {
            let start = Instant::now();
            let insert = "BEGIN; INSERT INTO communities VALUES ('c', 'm'); COMMIT;";
            writer.execute_batch(insert).unwrap();
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(communities(&db), 0);
        drop(db);

        assert!(commit.join().unwrap() >= Duration::from_millis(150));
        assert_eq!(communities(&read(&index).unwrap()), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// An index of this build's schema at `<case dir>/index.db`, then `sql` run over it, as a
    /// store could hand one over.
    #[cfg(feature = "snapshot")]
    fn crafted(case: &str, sql: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meada-vet-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let index = dir.join("index.db");
        let db = Connection::open(&index).unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.pragma_update(None, "user_version", super::schema_version())
            .unwrap();
        db.execute_batch(sql).unwrap();
        index
    }

    #[cfg(feature = "snapshot")]
    #[test]
    fn an_index_not_in_rollback_journal_mode_is_refused_before_it_is_opened() {
        let fix = Fix::Generation {
            id: "0123456789abcdef",
            dir: "generations/0123456789abcdef",
        };
        let index = crafted("wal", "PRAGMA journal_mode = WAL;");
        let e = super::vet(&index, REPO, fix).unwrap_err().to_string();
        assert!(e.starts_with(&format!("{REPO}: index.db: ")), "{e}");
        assert!(e.contains("rollback journal"), "{e}");
        for left in ["index.db-wal", "index.db-shm"] {
            assert!(!index.with_file_name(left).exists(), "{left}");
        }
        fs::write(&index, b"short").unwrap();
        assert!(super::vet(&index, REPO, fix).is_err());
    }

    #[cfg(feature = "snapshot")]
    #[test]
    fn an_index_with_a_schema_object_this_build_does_not_create_is_refused() {
        let fix = Fix::Generation {
            id: "0123456789abcdef",
            dir: "generations/0123456789abcdef",
        };
        assert!(super::vet(&crafted("clean", ""), REPO, fix).is_ok());
        let cases = [
            (
                "view",
                "CREATE VIEW endless AS WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 \
                 FROM n) SELECT i FROM n;",
                "view endless",
            ),
            (
                "symbols-view",
                "DROP TABLE symbols; CREATE VIEW symbols AS WITH RECURSIVE n(i) AS (SELECT 1 \
                 UNION ALL SELECT i + 1 FROM n) SELECT '' AS id, '' AS path, i AS start, i AS \
                 end FROM n;",
                "symbols",
            ),
            (
                "trigger",
                "CREATE TRIGGER t AFTER INSERT ON meta BEGIN DELETE FROM files; END;",
                "trigger t",
            ),
            (
                "altered",
                "DROP TABLE communities; CREATE TABLE communities (name TEXT, member TEXT, x);",
                "table communities",
            ),
            ("dropped", "DROP INDEX calls_path;", "index calls_path"),
        ];
        for (case, sql, object) in cases {
            let e = super::vet(&crafted(case, sql), REPO, fix)
                .unwrap_err()
                .to_string();
            assert!(e.starts_with(&format!("{REPO}: index.db: ")), "{case}: {e}");
            assert!(e.contains(object), "{case}: {e}");
        }
    }

    #[test]
    fn a_read_only_index_trusts_nothing_of_its_schema() {
        let dir = std::env::temp_dir().join(format!("meada-defensive-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let index = dir.join("index.db");
        Connection::open(&index)
            .unwrap()
            .execute_batch(SCHEMA)
            .unwrap();
        let db = super::read_only(&index).unwrap();
        let trusted: bool = (db.query_row("PRAGMA trusted_schema", [], |r| r.get(0))).unwrap();
        assert!(!trusted);
        let defensive = rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE;
        assert!(db.db_config(defensive).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    /// An index of another schema version at `index`, holding one community row, as an older
    /// Meada left it.
    fn older_index(index: &Path) {
        fs::create_dir_all(index.parent().unwrap()).unwrap();
        let db = Connection::open(index).unwrap();
        db.execute_batch(SCHEMA).unwrap();
        // An even version, which `schema_version` never is.
        db.execute_batch("INSERT INTO communities VALUES ('old', 'm'); PRAGMA user_version = 2;")
            .unwrap();
    }

    /// A checkout with one TypeScript file, for a rebuild to index.
    fn checkout(dir: &Path) -> PathBuf {
        let root = dir.join("checkout");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.ts"), "export function a() {\n  return 1;\n}\n").unwrap();
        root
    }

    /// The `files` rows of an index read through `db`.
    fn files(db: &Connection) -> i64 {
        (db.query_row("SELECT count(*) FROM files", [], |r| r.get(0))).unwrap()
    }

    /// A rebuild (spec #259) builds the new index beside the old one and renames it over it:
    /// a reader holding the old one keeps answering from it, a reader opening afterwards gets
    /// the whole new one, and no open meanwhile finds the index missing or empty.
    #[test]
    fn a_rebuild_replaces_the_index_by_rename_under_readers() {
        let (dir, index) = index_dir("rename");
        older_index(&index);
        let root = checkout(&dir);
        let before = Connection::open_with_flags(&index, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        before.execute_batch("BEGIN DEFERRED").unwrap();
        assert_eq!(communities(&before), 1);

        let done = Arc::new(AtomicBool::new(false));
        let watcher = std::thread::spawn({
            let (index, done) = (index.clone(), done.clone());
            move || {
                let mut seen = Vec::new();
                // One read more after the rebuild ends, so the new index is read at least once.
                loop {
                    let finished = done.load(Ordering::Relaxed);
                    let db = Connection::open_with_flags(&index, OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .unwrap();
                    let version: i32 =
                        (db.query_row("PRAGMA user_version", [], |r| r.get(0))).unwrap();
                    let rows = match version {
                        2 => communities(&db),
                        _ => files(&db),
                    };
                    seen.push((version == 2, rows));
                    if finished {
                        break seen;
                    }
                }
            }
        });
        let writer = open(&index, &root, REPO, &Serve::default()).unwrap();
        done.store(true, Ordering::Relaxed);
        let seen = watcher.join().unwrap();

        // The old index, read through the connection opened before the rename.
        assert_eq!(communities(&before), 1);
        assert_eq!(files(&writer), 1);
        assert_eq!(files(&read(&index).unwrap()), 1);
        assert!(seen.iter().all(|&(_, rows)| rows == 1), "{seen:?}");
        assert!(seen.iter().any(|&(old, _)| !old), "{seen:?}");
        drop(before);
        let mut left: Vec<String> = (fs::read_dir(index.parent().unwrap()).unwrap())
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["index.db", "index.db.lock"]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A second rebuild waits for the first one's lock, a missing index stays missing until
    /// the whole new one is renamed in, and the files a crashed rebuild left are removed only
    /// under that lock.
    #[test]
    fn a_rebuild_waits_for_the_rebuild_lock_and_then_removes_what_a_crash_left() {
        let (dir, index) = index_dir("lock");
        let root = checkout(&dir);
        let parent = index.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        let crashed = parent.join("index.db.new.1");
        fs::write(&crashed, "half an index").unwrap();
        fs::write(parent.join("index.db.new.1-journal"), "").unwrap();
        let held = fs::File::create(parent.join("index.db.lock")).unwrap();
        held.lock().unwrap();

        let rebuild = std::thread::spawn({
            let (index, root) = (index.clone(), root.clone());
            move || files(&open(&index, &root, REPO, &Serve::default()).unwrap())
        });
        std::thread::sleep(Duration::from_millis(300));
        assert!(!rebuild.is_finished());
        assert!(crashed.exists());
        assert!(!index.exists());
        held.unlock().unwrap();

        assert_eq!(rebuild.join().unwrap(), 1);
        let mut left: Vec<String> = (fs::read_dir(parent).unwrap())
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["index.db", "index.db.lock"]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// `<dir>/<name>/index.db`, an index of `REPO` holding one community `('c', 'm')`, with
    /// the hot journal a writer killed while changing its member to `x` leaves.
    fn interrupted(dir: &Path, name: &str) -> PathBuf {
        let index = dir.join(name).join("source/index.db");
        let writer = open(&index, dir, REPO, &Serve::default()).unwrap();
        writer
            .execute_batch("INSERT INTO communities VALUES ('c', 'm');")
            .unwrap();
        // Mid-transaction, the change reaches the file and the synced journal: a copy of both
        // is what a writer killed then leaves.
        let change = "BEGIN; UPDATE communities SET member = 'x';";
        writer.execute_batch(change).unwrap();
        writer.cache_flush().unwrap();
        let copy = dir.join(name);
        let journal = fs::read(index.with_file_name("index.db-journal")).unwrap();
        assert_ne!(
            journal.first(),
            Some(&0),
            "no hot journal: {} bytes",
            journal.len()
        );
        for name in ["index.db", "index.db-journal"] {
            fs::copy(index.with_file_name(name), copy.join(name)).unwrap();
        }
        copy.join("index.db")
    }

    /// A writer killed mid-commit leaves a hot journal only a write can roll back.
    #[test]
    fn a_read_only_open_over_an_interrupted_commit_names_the_repository_and_the_fix() {
        let (dir, _) = index_dir("hot");

        let error = read(&interrupted(&dir, "copy"))
            .err()
            .map(|e| e.to_string());

        assert_eq!(
            error.as_deref(),
            Some(
                "example.com/acme/web: its index holds an interrupted commit; run meada index \
                 --workspace ops/meada.workspace.toml"
            )
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The same on a read-only mount: the directory and files cannot be written, and the open
    /// still names the interrupted commit, not a missing file or a permission error.
    #[cfg(unix)]
    #[test]
    fn a_read_only_open_over_an_interrupted_commit_on_a_read_only_mount_names_the_fix() {
        use std::os::unix::fs::PermissionsExt;
        /// The index's directory read-only (`0555`, its files `0444`) until dropped, even on a
        /// panic, so the temp dir can be removed.
        struct Mount(PathBuf);
        impl Mount {
            fn set(&self, dir: u32, file: u32) {
                for name in ["index.db", "index.db-journal"] {
                    let file = fs::Permissions::from_mode(file);
                    let _ = fs::set_permissions(self.0.join(name), file);
                }
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(dir));
            }
        }
        impl Drop for Mount {
            fn drop(&mut self) {
                self.set(0o755, 0o644);
            }
        }

        let (dir, _) = index_dir("hot_mount");
        let index = interrupted(&dir, "copy");
        let folder = index.parent().unwrap().to_path_buf();
        let mount = Mount(folder.clone());
        mount.set(0o555, 0o444);
        if fs::File::create(folder.join("probe")).is_ok() {
            eprintln!("skipped: permissions do not bind this user (root)");
            drop(mount);
            let _ = fs::remove_dir_all(&dir);
            return;
        }

        let error = read(&index).err().map(|e| e.to_string());
        let left = fs::read_dir(&folder).unwrap().count();

        drop(mount);
        assert_eq!(
            error.as_deref(),
            Some(
                "example.com/acme/web: its index holds an interrupted commit; run meada index \
                 --workspace ops/meada.workspace.toml"
            )
        );
        // `source/`, the index and its journal: nothing was added.
        assert_eq!(left, 3);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The writable open rolls back the hot journal of an index of the current schema and keeps
    /// that index: no rebuild, so no rebuild lock either.
    #[test]
    fn a_writable_open_rolls_back_a_hot_journal_and_keeps_the_index() {
        let (dir, _) = index_dir("rollback");
        let index = interrupted(&dir, "copy");

        let db = open(&index, &dir, REPO, &Serve::default()).unwrap();

        let member: String =
            (db.query_row("SELECT member FROM communities", [], |r| r.get(0))).unwrap();
        assert_eq!(member, "m");
        assert!(!index.with_file_name("index.db-journal").exists());
        assert!(!index.with_file_name("index.db.lock").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A commit in progress past the busy timeout is an error for a writer too, never a reason
    /// to rebuild: the commit lands in the index afterwards.
    #[test]
    fn a_writable_open_of_an_index_locked_past_the_busy_timeout_fails_and_rebuilds_nothing() {
        let (dir, index) = index_dir("writer_busy");
        let writer = open(&index, &dir, REPO, &Serve::default()).unwrap();
        (writer.execute_batch("BEGIN EXCLUSIVE; INSERT INTO communities VALUES ('c', 'm');"))
            .unwrap();

        let second = with_busy_timeout(Duration::from_millis(50), || {
            open(&index, &dir, REPO, &Serve::default())
        });

        assert!(second.is_err());
        writer.execute_batch("COMMIT").unwrap();
        assert_eq!(communities(&read(&index).unwrap()), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A journal left beside a missing index, or beside one of another schema, is not the new
    /// index's: the rebuild removes it under the lock, and the rebuilt index opens clean.
    #[test]
    fn a_rebuild_removes_a_journal_left_beside_the_index() {
        let (dir, index) = index_dir("stray_journal");
        // Taken before the checkout exists, so the journal's pages hold no file.
        let hot = interrupted(&dir, "hot");
        let root = checkout(&dir);
        let journal = fs::read(hot.with_file_name("index.db-journal")).unwrap();
        let parent = index.parent().unwrap();
        for older in [false, true] {
            let _ = fs::remove_dir_all(parent);
            fs::create_dir_all(parent).unwrap();
            if older {
                older_index(&index);
            }
            fs::write(index.with_file_name("index.db-journal"), &journal).unwrap();

            let db = open(&index, &root, REPO, &Serve::default()).unwrap();

            // The journal's pages (the community row) never reach the new index.
            assert_eq!(communities(&db), 0, "older: {older}");
            assert_eq!(files(&db), 1, "older: {older}");
            assert_eq!(files(&read(&index).unwrap()), 1, "older: {older}");
            assert!(!index.with_file_name("index.db-journal").exists());
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// A rebuild lock that cannot be taken (a read-only index directory) is named.
    #[test]
    fn a_rebuild_lock_that_cannot_be_taken_is_named() {
        let (dir, index) = index_dir("lock_error");
        let lock = index.with_file_name("index.db.lock");
        fs::create_dir_all(&lock).unwrap();

        let error = open(&index, &dir, REPO, &Serve::default()).err().unwrap();

        let expected = format!("cannot take the rebuild lock `{}`: ", lock.display());
        assert!(error.to_string().starts_with(&expected), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

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
            |abs| fs::read(abs),
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

    #[test]
    fn a_config_file_is_read_up_to_one_byte_past_the_size_limit() {
        let root = std::env::temp_dir().join(format!("meada-index-config-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("tsconfig.json"), " ".repeat(lang::MAX_BYTES * 2)).unwrap();
        fs::write(root.join("jsconfig.json"), "{}").unwrap();

        let large = read_config(&root, "tsconfig.json").map(|src| src.len());
        let small = read_config(&root, "jsconfig.json");
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(large, Some(lang::MAX_BYTES + 1));
        assert_eq!(small.as_deref(), Some(&b"{}"[..]));
    }

    /// What an index stores: `SCHEMA`, then the facts each pack stores for its sample in
    /// `tests/fixtures/schema/`, then the summary of each provider document and manifest sample.
    /// Its golden is the schema version's source (`schema_version`), so a change fails until
    /// `MEADA_BLESS=1` rewrites the golden, which changes the version.
    #[test]
    fn the_stored_format_matches_its_golden() {
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
        let summary =
            serde_json::to_string_pretty(&package::read_manifest("package.json", &src)).unwrap();
        actual += &format!("\nsample.package.json\n{summary}\n");
        for (name, path) in [
            ("sample.Cargo.toml", "Cargo.toml"),
            ("sample.pyproject.toml", "pyproject.toml"),
            ("sample.requirements.txt", "requirements.txt"),
        ] {
            let src = fs::read(tests.join("fixtures/schema").join(name)).unwrap();
            let summary = package::read_manifest(path, &src);
            let summary = serde_json::to_string_pretty(&summary).unwrap();
            actual += &format!("\n{name}\n{summary}\n");
        }
        let src = fs::read(tests.join("fixtures/schema/sample.tsconfig.json")).unwrap();
        let summary = serde_json::to_string_pretty(&package::read_tsconfig(&src)).unwrap();
        actual += &format!("\nsample.tsconfig.json\n{summary}\n");

        let golden = tests.join("golden/schema.txt");
        if std::env::var_os("MEADA_BLESS").is_some_and(|v| v == "1") {
            fs::write(&golden, actual).unwrap();
            return;
        }
        let expected = fs::read_to_string(&golden).unwrap();
        assert!(
            actual == expected,
            "the stored format differs from {}: rerun with MEADA_BLESS=1 to rewrite it, which \
             changes the schema version\n--- actual ---\n{actual}",
            golden.display()
        );
    }

    /// Each file under `dir` with its bytes and mtime, sorted.
    #[cfg(feature = "snapshot")]
    fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut all = Vec::new();
        let mut dirs = vec![dir.to_path_buf()];
        while let Some(at) = dirs.pop() {
            for entry in fs::read_dir(&at).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                match meta.is_dir() {
                    true => dirs.push(path),
                    false => all.push((
                        path.clone(),
                        fs::read(&path).unwrap(),
                        meta.modified().unwrap(),
                    )),
                }
            }
        }
        all.sort();
        all
    }

    /// The freshness check of `meada snapshot publish` (ADR 0012): it sees what a refresh would
    /// change (an edited, added or removed file, an edited manifest, another identity) and
    /// nothing else, and writes nothing.
    #[cfg(feature = "snapshot")]
    #[test]
    fn fresh_sees_what_a_refresh_would_change_and_writes_nothing() {
        use super::{fresh, refresh};
        let (dir, index) = index_dir("fresh");
        let root = checkout(&dir);
        fs::write(root.join("package.json"), "{\"name\": \"web\"}\n").unwrap();
        let serve = Serve::default();
        let index_now = || {
            let db = open(&index, &root, REPO, &serve).unwrap();
            refresh(&db, &root, REPO, &serve).unwrap();
        };
        let is_fresh = |identity: &str| {
            let db = read(&index).unwrap();
            fresh(&db, &root, identity, &serve).unwrap()
        };
        // Just written, so every mtime is too recent to trust: the hashes answer.
        index_now();
        let before = tree(&dir);
        assert!(is_fresh(REPO));
        assert!(!is_fresh("example.com/acme/other"));
        assert_eq!(tree(&dir), before, "fresh wrote nothing");

        // A touched file of the same content is no difference.
        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        let a = fs::File::options()
            .write(true)
            .open(root.join("a.ts"))
            .unwrap();
        a.set_modified(later).unwrap();
        assert!(is_fresh(REPO));

        let edits: [(&str, Option<&str>); 4] = [
            ("a.ts", Some("export function b() {}\n")),
            ("b.ts", Some("export const c = 1;\n")),
            ("b.ts", None),
            ("package.json", Some("{\"name\": \"site\"}\n")),
        ];
        for (file, content) in edits {
            match content {
                Some(content) => fs::write(root.join(file), content).unwrap(),
                None => fs::remove_file(root.join(file)).unwrap(),
            }
            assert!(!is_fresh(REPO), "{file} {content:?}");
            index_now();
            assert!(is_fresh(REPO), "{file} {content:?} indexed");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// The same for the documents of a `[serve]` table: one found by name or listed in
    /// `serve.documents`, edited, added or removed, is a difference.
    #[cfg(feature = "snapshot")]
    #[test]
    fn fresh_sees_an_edited_added_or_removed_document() {
        use super::{fresh, refresh};
        let (dir, index) = index_dir("fresh_documents");
        let root = checkout(&dir);
        let spec = |title: &str| format!("openapi: 3.0.0\ninfo:\n  title: {title}\npaths: {{}}\n");
        fs::write(root.join("openapi.yaml"), spec("web")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        let serve = Serve {
            declared: true,
            documents: vec!["docs/api.yaml".into()],
            ..Serve::default()
        };
        let index_now = || {
            let db = open(&index, &root, REPO, &serve).unwrap();
            refresh(&db, &root, REPO, &serve).unwrap();
        };
        let is_fresh = || fresh(&read(&index).unwrap(), &root, REPO, &serve).unwrap();
        index_now();
        assert!(is_fresh());
        let edits: [(&str, Option<String>); 4] = [
            ("openapi.yaml", Some(spec("site"))),
            ("docs/api.yaml", Some(spec("api"))),
            ("docs/api.yaml", None),
            ("openapi.yaml", None),
        ];
        for (file, content) in edits {
            match &content {
                Some(content) => fs::write(root.join(file), content).unwrap(),
                None => fs::remove_file(root.join(file)).unwrap(),
            }
            assert!(!is_fresh(), "{file} {content:?}");
            index_now();
            assert!(is_fresh(), "{file} {content:?} indexed");
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
