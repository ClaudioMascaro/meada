use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use rusqlite::config::DbConfig;
use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::Serialize;

use crate::Result;
use crate::community::{self, Community};
use crate::config::Serve;
use crate::config_values;
use crate::contract::{self, Found, Level, Link, Names, Role, Site};
use crate::decisions::Stale;
use crate::document::{self, Summary};
use crate::entry::{self, EntryKind, EntryPoint};
use crate::iac;
use crate::kube;
use crate::lang::{self, Facts};
use crate::msg::{Fix, Msg};
use crate::package::{self, Config, Dependency, Hop, Import, ImportCall, Manifest, Package, lock};
use crate::repo;
use crate::resolve::{Ancestry, Code, Exported, Flexible, Resolution, Resolver, Target};
use crate::shape::{Shape, Want};
pub(crate) use meada_core::export as written;
pub(crate) use meada_core::export::{Binding, CallSite, File, Process, Publish, Span, Symbol};

mod values;

#[cfg(test)]
const EXTRACTION_REVISION: u32 = 10;

pub(crate) fn schema_version() -> i32 {
    let hash = blake3::hash(include_bytes!("../tests/golden/schema.txt"));
    let [a, b, c, d, ..] = *hash.as_bytes();
    i32::from_le_bytes([a, b, c, d]) | 1
}

pub(crate) const SCHEMA: &str = "
CREATE TABLE meta (identity TEXT NOT NULL, configs TEXT NOT NULL);
CREATE TABLE files (
    path TEXT PRIMARY KEY, hash TEXT NOT NULL, mtime INTEGER, size INTEGER NOT NULL,
    facts TEXT NOT NULL, resolved TEXT NOT NULL, setting TEXT NOT NULL
);
CREATE TABLE symbols (id TEXT NOT NULL, path TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL, hash TEXT NOT NULL);
CREATE INDEX symbols_path ON symbols (path);
CREATE INDEX symbols_id ON symbols (id);
CREATE INDEX symbols_hash ON symbols (hash);
CREATE TABLE calls (
    id TEXT NOT NULL, caller TEXT NOT NULL, target TEXT, level TEXT,
    path TEXT NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL, narrowed INTEGER NOT NULL,
    decorates TEXT, kind TEXT, holder TEXT
);
CREATE INDEX calls_path ON calls (path);
CREATE INDEX calls_id ON calls (id);
CREATE INDEX calls_caller ON calls (caller);
CREATE INDEX calls_target ON calls (target);
CREATE TABLE candidates (call TEXT NOT NULL, target TEXT NOT NULL);
CREATE INDEX candidates_call ON candidates (call);
CREATE INDEX candidates_target ON candidates (target);
CREATE TABLE contracts (
    role TEXT NOT NULL, contract TEXT, site TEXT NOT NULL, via TEXT, path TEXT NOT NULL,
    method TEXT, fragments TEXT, shape TEXT, identity TEXT, lead TEXT, cap TEXT, client TEXT
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
CREATE TABLE exports (path TEXT NOT NULL, name TEXT, file TEXT, target TEXT, source TEXT);
CREATE INDEX exports_path ON exports (path);
CREATE TABLE families (path TEXT NOT NULL, class TEXT NOT NULL, ancestors TEXT);
CREATE TABLE flexible (path TEXT NOT NULL, name TEXT NOT NULL, least INTEGER NOT NULL, most INTEGER);
CREATE TABLE entry_points (identity TEXT NOT NULL, kind TEXT NOT NULL, label TEXT NOT NULL, site TEXT NOT NULL);
CREATE TABLE communities (name TEXT NOT NULL, member TEXT NOT NULL);
CREATE INDEX communities_member ON communities (member);
CREATE TABLE valued (path TEXT PRIMARY KEY);
CREATE TABLE config_files (
    path TEXT PRIMARY KEY, hash TEXT NOT NULL, mtime INTEGER, size INTEGER NOT NULL,
    summary TEXT NOT NULL
);
CREATE TABLE taken (path TEXT PRIMARY KEY);
CREATE TABLE dockerfiles (folder TEXT PRIMARY KEY);
CREATE TABLE contract_configs (
    path TEXT PRIMARY KEY, hash TEXT NOT NULL, mtime INTEGER, size INTEGER NOT NULL,
    summary TEXT NOT NULL, inputs TEXT NOT NULL DEFAULT ''
);
CREATE TABLE config_values (
    id INTEGER PRIMARY KEY, path TEXT NOT NULL, key TEXT NOT NULL, value TEXT, type TEXT NOT NULL, line INTEGER NOT NULL,
    cut INTEGER NOT NULL, folded_key TEXT NOT NULL, folded_value TEXT
);
CREATE INDEX config_values_path ON config_values (path);
CREATE INDEX config_values_folded_key ON config_values (folded_key);
CREATE INDEX config_values_folded_value ON config_values (folded_value);
CREATE VIRTUAL TABLE config_search USING fts5(
    folded_key, folded_value, content='config_values', content_rowid='id',
    tokenize='trigram case_sensitive 1'
);
CREATE TABLE iac_nodes (
    address TEXT NOT NULL, root TEXT NOT NULL, type TEXT NOT NULL, name TEXT,
    pattern INTEGER NOT NULL, overridable TEXT NOT NULL, path TEXT NOT NULL,
    line INTEGER NOT NULL, end_line INTEGER NOT NULL, block TEXT NOT NULL,
    image TEXT, image_overridable INTEGER, build TEXT, build_overridable INTEGER, role TEXT,
    handler TEXT, handler_overridable INTEGER, events TEXT NOT NULL, build_image INTEGER NOT NULL,
    subscription TEXT, environment TEXT NOT NULL, images TEXT NOT NULL, provisioned TEXT,
    ingress TEXT NOT NULL, routes TEXT NOT NULL, dockerfile TEXT, actions TEXT
);
CREATE INDEX iac_nodes_address ON iac_nodes (address);
CREATE INDEX iac_nodes_path ON iac_nodes (path);
CREATE TABLE iac_references (
    root TEXT NOT NULL, source TEXT NOT NULL, target TEXT NOT NULL, via TEXT
);
CREATE INDEX iac_references_source ON iac_references (root, source);
CREATE INDEX iac_references_target ON iac_references (root, target);
CREATE TABLE iac_images (
    root TEXT NOT NULL, address TEXT NOT NULL, container TEXT NOT NULL, image TEXT NOT NULL
);
CREATE INDEX iac_images_address ON iac_images (root, address);
CREATE TABLE iac_unresolved (
    root TEXT NOT NULL, path TEXT NOT NULL, entry TEXT NOT NULL, reason TEXT NOT NULL
);
CREATE TABLE lockfiles (
    path TEXT PRIMARY KEY, hash TEXT NOT NULL, mtime INTEGER, size INTEGER NOT NULL,
    state TEXT NOT NULL
);
CREATE TABLE resolved (
    lockfile TEXT NOT NULL, manifest TEXT NOT NULL, name TEXT NOT NULL, origin TEXT NOT NULL,
    version TEXT NOT NULL
);
CREATE INDEX resolved_lockfile ON resolved (lockfile);
";

const RACY: Duration = Duration::from_secs(2);

pub fn path(root: &Path) -> PathBuf {
    root.join(".meada/index.db")
}

pub enum State {
    Missing,
    OtherSchema,
    Locked,
    Indexed { files: i64 },
}

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

fn provenance_fingerprint(db: &Connection) -> Result<String> {
    let mut hash = blake3::Hasher::new();
    for query in [
        "SELECT identity, configs FROM meta ORDER BY identity",
        "SELECT path, hash FROM files ORDER BY path",
        "SELECT path, hash FROM documents ORDER BY path",
        "SELECT path, hash FROM manifests ORDER BY path",
        "SELECT path, hash FROM config_files ORDER BY path",
        "SELECT path, hash FROM contract_configs ORDER BY path",
    ] {
        hash.update(query.as_bytes());
        let mut statement = db.prepare(query)?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let bytes = serde_json::to_vec(&row?)?;
            hash.update(&bytes);
        }
    }
    Ok(hash.finalize().to_hex().to_string())
}

pub(crate) fn record_commit(db: &Connection, index: &Path, head: Option<&str>) -> Result<()> {
    let file = index.with_extension("commit.json");
    let Some(head) = head else {
        if file.exists() {
            fs::remove_file(&file)?;
        }
        return Ok(());
    };
    let bytes = serde_json::to_vec(
        &serde_json::json!({"head": head, "fingerprint": provenance_fingerprint(db)?}),
    )?;
    let temporary = file.with_extension(format!("json.new.{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, &file)?;
    Ok(())
}

// ponytail: the optional provenance sidecar contains two hashes only; a longer document is
// untrusted and has no indexed commit.
const PROVENANCE_MAX: u64 = 4096;

#[cfg(feature = "ui")]
pub(crate) fn provenance(index: &Path) -> Result<(i32, Option<String>)> {
    let db = read_only(index)?;
    db.execute_batch("BEGIN")?;
    let schema = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok((schema, indexed_commit(&db, index)))
}

pub(crate) fn indexed_commit(db: &Connection, index: &Path) -> Option<String> {
    let bytes =
        crate::config::read_bounded(&index.with_extension("commit.json"), PROVENANCE_MAX).ok()?;
    let written: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let head = written["head"].as_str()?;
    if ![40, 64].contains(&head.len())
        || !head
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    (written["fingerprint"].as_str()? == provenance_fingerprint(db).ok()?).then(|| head.to_string())
}

const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

thread_local! {
    static BUSY: std::cell::Cell<Duration> = const { std::cell::Cell::new(BUSY_TIMEOUT) };
    static RUNNING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TIMEOUT_FROM: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

// ponytail: a run in flight on any index extends the wait, since a plain function knows no
// connection; a wait on another process's lock then lasts until the runs of this process end.
fn wait(tries: i32) -> bool {
    let in_flight = !RUNNING.get() && IN_FLIGHT.load(Ordering::Relaxed) > 0;
    let since = match TIMEOUT_FROM.get() {
        Some(since) if tries > 0 && !in_flight => since,
        _ => Instant::now(),
    };
    TIMEOUT_FROM.set(Some(since));
    if since.elapsed() >= BUSY.get() {
        return false;
    }
    std::thread::sleep(Duration::from_millis(
        u64::from(tries.clamp(0, 49).unsigned_abs()) + 1,
    ));
    true
}

/// Runs `f` with `timeout` as this thread's busy timeout, instead of 10 seconds: the most its
/// read-only opens wait for a commit in progress, and what its statements on a connection opened
/// to write wait before only a refresh in flight keeps them waiting (`wait`). No option or
/// setting reaches it: tests, and the readiness probe of `status --workspace`.
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

fn read_only(path: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    db.pragma_update(None, "trusted_schema", false)?;
    db.busy_timeout(BUSY.get())?;
    Ok(db)
}

#[cfg(feature = "snapshot")]
fn objects(db: &Connection) -> rusqlite::Result<Vec<[Option<String>; 4]>> {
    let mut statement =
        db.prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY 1, 2, 3, 4")?;
    let rows = statement.query_map([], |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?]))?;
    rows.collect()
}

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

pub fn open_read(index: &Path, repo: &str, fix: Fix) -> Result<Connection> {
    let db = read_only(index).map_err(|e| unreadable(e, index, repo, fix))?;
    db.execute_batch("BEGIN DEFERRED")
        .map_err(|e| unreadable(e, index, repo, fix))?;
    check_read(&db, index, repo, fix)?;
    Ok(db)
}

fn check_read(db: &Connection, index: &Path, repo: &str, fix: Fix) -> Result<()> {
    let version = (db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0)))
        .map_err(|e| unreadable(e, index, repo, fix))?;
    if version != schema_version() {
        return Err(Msg::IndexOtherVersion { repo, fix }.text().into());
    }
    Ok(())
}

fn unreadable(
    e: rusqlite::Error,
    index: &Path,
    repo: &str,
    fix: Fix,
) -> Box<dyn std::error::Error> {
    let text = match e.sqlite_error_code() {
        Some(ErrorCode::CannotOpen) => Msg::IndexMissing { repo, fix }.text(),
        Some(ErrorCode::NotADatabase) => Msg::IndexOtherVersion { repo, fix }.text(),
        Some(ErrorCode::ReadOnly) => Msg::IndexInterrupted { repo, fix }.text(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Msg::IndexLocked {
            repo,
            index: &index.display().to_string(),
            timeout: BUSY.get(),
        }
        .text(),
        _ => format!("{repo}: {e}"),
    };
    text.into()
}

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

fn current(index: &Path) -> Result<Option<Connection>> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let db = match Connection::open_with_flags(index, flags) {
        Err(e) if e.sqlite_error_code() == Some(ErrorCode::CannotOpen) => return Ok(None),
        db => db?,
    };
    db.busy_handler(Some(wait))?;
    let version = db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0));
    db.busy_handler(Some(wait))?;
    match version {
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

fn beside(index: &Path, suffix: &str) -> PathBuf {
    let mut name = index.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

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
    match fs::remove_file(beside(index, "-journal")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let new = beside(index, &format!(".new.{}", std::process::id()));
    let built = (|| -> Result<()> {
        let db = writable(&new)?;
        db.execute_batch(SCHEMA)?;
        db.pragma_update(None, "user_version", schema_version())?;
        refresh(&db, root, identity, &|| Ok(serve.clone()))
    })();
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

fn writable(path: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open(path)?;
    db.busy_handler(Some(wait))?;
    Ok(db)
}

struct Stored {
    hash: String,
    mtime: Option<i64>,
    size: i64,
    resolved: String,
    setting: String,
}

struct Stamp {
    hash: String,
    mtime: Option<i64>,
    size: i64,
}

struct Parsed {
    path: String,
    hash: String,
    mtime: Option<i64>,
    size: i64,
    facts: Facts,
}

#[derive(Serialize)]
struct Resolved {
    id: String,
    caller: String,
    target: Option<String>,
    level: Option<Level>,
    start: i64,
    end: i64,
    candidates: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    narrowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    decorates: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    holder: Option<String>,
}

#[derive(Serialize)]
struct Rows<'a> {
    symbols: Vec<(String, i64, i64, &'a str)>,
    calls: Vec<Resolved>,
}

pub struct Indexed<'a> {
    pub manifests: &'a [Manifest],
    pub tsconfigs: &'a [Config],
    pub exported: &'a [Exported],
    pub files: &'a [&'a str],
    pub configs: &'a [(&'a str, &'a [u8])],
    pub symlinks: &'a [&'a str],
    pub channel_clients: &'a [crate::config::ChannelClient],
}

pub fn refresh(
    db: &Connection,
    root: &Path,
    identity: &str,
    read_serve: &dyn Fn() -> Result<Serve>,
) -> Result<()> {
    let Some(path) = db.path().filter(|p| !p.is_empty()) else {
        return refresh_keeping(db, root, identity, read_serve, true);
    };
    let flight = {
        let mut flights = FLIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
        flights.entry(PathBuf::from(path)).or_default().clone()
    };
    let mut runs = flight.runs.lock().unwrap_or_else(PoisonError::into_inner);
    let needed = runs.started + 1;
    while runs.running && runs.finished < needed {
        runs = flight
            .done
            .wait(runs)
            .unwrap_or_else(PoisonError::into_inner);
    }
    if runs.finished >= needed {
        return runs.failed.clone().map_or(Ok(()), |e| Err(e.into()));
    }
    runs.running = true;
    runs.started += 1;
    IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
    let mut run = Run {
        flight: &flight,
        number: runs.started,
        failed: Some("the refresh in flight stopped".to_string()),
        nested: RUNNING.replace(true),
    };
    drop(runs);
    let outcome = refresh_keeping(db, root, identity, read_serve, true);
    run.failed = outcome.as_ref().err().map(|e| e.to_string());
    outcome
}

static FLIGHTS: Mutex<BTreeMap<PathBuf, Arc<Flight>>> = Mutex::new(BTreeMap::new());

#[derive(Default)]
struct Flight {
    runs: Mutex<Runs>,
    done: Condvar,
}

#[derive(Default)]
struct Runs {
    started: u64,
    finished: u64,
    running: bool,
    failed: Option<String>,
}

struct Run<'a> {
    flight: &'a Flight,
    number: u64,
    failed: Option<String>,
    nested: bool,
}

impl Drop for Run<'_> {
    fn drop(&mut self) {
        RUNNING.set(self.nested);
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        let mut runs = (self.flight.runs.lock()).unwrap_or_else(PoisonError::into_inner);
        runs.running = false;
        runs.finished = self.number;
        runs.failed = self.failed.take();
        self.flight.done.notify_all();
    }
}

// ponytail: a refresh another process's writer refuses walks the tree again, up to this many
// times, before it takes the write lock first and holds it through its walk; a queue of waiting
// writers would walk once.
/// Deferred attempts before a refresh takes the write lock first (`attempts`).
const DEFERRED_ATTEMPTS: usize = 3;

fn refresh_keeping(
    db: &Connection,
    root: &Path,
    identity: &str,
    read_serve: &dyn Fn() -> Result<Serve>,
    keep: bool,
) -> Result<()> {
    attempts(db, |behavior| {
        refresh_attempt(db, root, identity, read_serve, keep, behavior)
    })
}

fn attempts(
    db: &Connection,
    mut attempt: impl FnMut(TransactionBehavior) -> Result<()>,
) -> Result<()> {
    for _ in 0..DEFERRED_ATTEMPTS {
        match attempt(TransactionBehavior::Deferred) {
            Err(e) if busy(&*e) => db.execute_batch("BEGIN IMMEDIATE; ROLLBACK")?,
            done => return done,
        }
    }
    attempt(TransactionBehavior::Immediate)
}

fn busy(e: &(dyn std::error::Error + 'static)) -> bool {
    (e.downcast_ref::<rusqlite::Error>()).is_some_and(|e| {
        matches!(
            e.sqlite_error_code(),
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
        )
    })
}

fn refresh_attempt(
    db: &Connection,
    root: &Path,
    identity: &str,
    read_serve: &dyn Fn() -> Result<Serve>,
    keep: bool,
    behavior: TransactionBehavior,
) -> Result<()> {
    let now = SystemTime::now();
    let tx = Transaction::new_unchecked(db, behavior)?;
    let known = tx
        .query_row("SELECT identity, configs FROM meta", [], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .optional()?;
    let known_configs = match known {
        Some((id, configs)) if id == identity => Some(configs),
        _ => {
            tx.execute_batch(
                "DELETE FROM meta; DELETE FROM files; DELETE FROM symbols; \
                 DELETE FROM calls; DELETE FROM candidates; DELETE FROM contracts; \
                 DELETE FROM documents; DELETE FROM manifests; DELETE FROM exports; \
                 DELETE FROM families; DELETE FROM flexible; DELETE FROM entry_points; DELETE FROM communities; \
                 DELETE FROM valued; DELETE FROM config_files; DELETE FROM taken; DELETE FROM dockerfiles; DELETE FROM contract_configs; DELETE FROM config_values; \
                 INSERT INTO config_search (config_search) VALUES ('delete-all'); \
                 DELETE FROM iac_nodes; DELETE FROM iac_references; DELETE FROM iac_images; \
                 DELETE FROM iac_unresolved; DELETE FROM lockfiles; DELETE FROM resolved;",
            )?;
            None
        }
    };
    let config = crate::config::load(root)?;
    let helm_env: Vec<String> = config.helm_env.iter().map(|h| h.key.clone()).collect();
    let mappings = config.iac;
    let serve = read_serve()?;
    let mapping_stamp = mappings.stamp()?;
    let mappings_changed = known_configs
        .as_deref()
        .and_then(|stamp| stamp.rsplit_once(':'))
        .is_none_or(|(_, stamp)| stamp != mapping_stamp);
    let stored = stored_files(&tx)?;

    let all = walk(root);
    let documents = documents_found(root, &all, &serve);
    refresh_stamped(
        &tx,
        "documents",
        &documents,
        now,
        |_| Ok(false),
        |abs| read_up_to(abs, document::DOCUMENT_BYTES_MAX),
        |path, src| Ok(serde_json::to_string(&document::read(path, src))?),
    )?;
    refresh_stamped(
        &tx,
        "manifests",
        &manifests_found(&all),
        now,
        |_| Ok(false),
        |abs| read_up_to(abs, lang::MAX_BYTES),
        |path, src| match package::is_tsconfig(path) {
            true => Ok(serde_json::to_string(&package::read_tsconfig(src))?),
            false => Ok(serde_json::to_string(&package::read_manifest(path, src))?),
        },
    )?;
    let gone = refresh_stamped(
        &tx,
        "lockfiles",
        &lockfiles_found(&all),
        now,
        |_| Ok(false),
        |abs| read_up_to(abs, lock::MAX_LOCK_BYTES),
        |path, src| resolve_lockfile(&tx, root, path, src),
    )?;
    for path in gone {
        tx.execute("DELETE FROM resolved WHERE lockfile = ?1", params![path])?;
    }
    let hidden = hidden_ci(root);
    let (configs, shadowed) = configs_found(&all, &hidden);
    let sensitive = config_values::Sensitive::new(&tx, &configs, &shadowed);
    let Packs {
        walked,
        packs,
        configs: pack_files,
        symlinks,
        digest,
        settings,
    } = packs_of(root, &all);
    let taken: BTreeSet<&str> = (documents.iter().map(|(path, _)| path.as_str()))
        .chain(pack_files.values().flatten().map(|(path, _)| path.as_str()))
        .collect();
    let kind_file =
        |path: &str, src: &[u8]| config_values::kind_file(path, src, |p| taken.contains(p));
    let before: BTreeSet<String> = (tx.prepare("SELECT path FROM taken")?)
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let terraform = std::cell::RefCell::new(BTreeSet::new());
    let kubernetes = std::cell::Cell::new(false);
    let located: HashMap<&str, &PathBuf> = configs.iter().map(|(p, abs)| (&**p, abs)).collect();
    let actions = |path: &str, src: Option<&[u8]>| -> Result<()> {
        if !iac::actions::is_file(path) {
            return Ok(());
        }
        let twin = iac::actions::twin(path).and_then(|twin| {
            let abs = located.get(twin.as_str())?;
            Some((twin, read_up_to(abs, lang::MAX_BYTES).ok()?))
        });
        iac::actions::refresh(&tx, path, src, twin.is_some())?;
        if let Some((twin, bytes)) = &twin {
            iac::actions::refresh(&tx, twin, Some(bytes), src.is_some())?;
        }
        Ok(())
    };
    let gone = refresh_stamped(
        &tx,
        "config_files",
        &configs,
        now,
        |path| Ok(sensitive.stale(path)? || before.contains(path) != taken.contains(path)),
        |abs| read_up_to(abs, lang::MAX_BYTES),
        |path, src| {
            if iac::is_input(path) {
                terraform
                    .borrow_mut()
                    .insert(config_values::parent(path).to_string());
            }
            if kube::changed(&tx, path, Some(src))? {
                kubernetes.set(true);
            }
            if iac::template_changed(&tx, path, Some(src))? {
                iac::refresh_template(&tx, path, Some(src))?;
            }
            actions(path, Some(src))?;
            let whole = || kind_file(path, src);
            let (outcome, rows) = config_values::read_with(path, src, &sensitive.of(path)?, whole);
            config_values::store(&tx, path, &rows)?;
            Ok(serde_json::to_string(&outcome)?)
        },
    )?;
    let mut kube_left = false;
    for path in &gone {
        kube_left = kube_left || kube::changed(&tx, path, None)?;
        if iac::template_changed(&tx, path, None)? {
            iac::refresh_template(&tx, path, None)?;
        }
        actions(path, None)?;
        config_values::forget(&tx, path)?;
    }
    if !before.iter().map(String::as_str).eq(taken.iter().copied()) {
        tx.execute("DELETE FROM taken", [])?;
        for path in &taken {
            tx.execute("INSERT INTO taken VALUES (?1)", [path])?;
        }
    }
    let dockerfiles: BTreeSet<&str> = (all.iter().chain(&hidden))
        .filter_map(|(path, _)| {
            let folder =
                (path.strip_suffix("Dockerfile")).or_else(|| path.strip_suffix("dockerfile"))?;
            if folder.is_empty() {
                Some("")
            } else {
                folder.strip_suffix('/')
            }
        })
        .collect();
    if stored_dockerfiles(&tx)?
        .iter()
        .ne(dockerfiles.iter().copied())
    {
        tx.execute("DELETE FROM dockerfiles", [])?;
        for folder in &dockerfiles {
            tx.execute("INSERT INTO dockerfiles VALUES (?1)", [folder])?;
        }
    }
    for (path, abs) in &configs {
        let Some(names) = sensitive.merged(path)? else {
            continue;
        };
        (terraform.borrow_mut()).insert(config_values::parent(path).to_string());
        let Ok(src) = read_up_to(abs, lang::MAX_BYTES) else {
            continue;
        };
        let (outcome, rows) =
            config_values::read_with(path, &src, &names, || kind_file(path, &src));
        config_values::store(&tx, path, &rows)?;
        tx.execute(
            "UPDATE config_files SET summary = ?2 WHERE path = ?1",
            params![path, serde_json::to_string(&outcome)?],
        )?;
    }
    let mut dirs = terraform.into_inner();
    if mappings_changed {
        dirs.extend(
            configs
                .iter()
                .filter(|(p, _)| iac::is_input(p))
                .map(|(p, _)| config_values::parent(p).to_string()),
        );
    }
    let left = gone.iter().filter(|p| iac::is_input(p));
    dirs.extend(left.map(|p| config_values::parent(p).to_string()));
    // ponytail: a changed Terraform file reads every Terraform file of the repository again to
    // find the roots and their units, though only the units holding it are evaluated again.
    if !dirs.is_empty() {
        let mut files = Vec::new();
        for (path, abs) in configs.iter().filter(|(p, _)| iac::is_input(p)) {
            if let Ok(src) = read_up_to(abs, lang::MAX_BYTES) {
                files.push((path.clone(), src));
            }
        }
        let repository = iac::Repository::read_with(&files, &shadowed, &mappings);
        iac::refresh(&tx, &repository, &dirs)?;
    }
    // ponytail: one changed Kubernetes file evaluates every overlay and directory of manifests of
    // the repository again, not only the units it belongs to; a repository of many overlays
    // would make that slow.
    if kubernetes.get() || kube_left || mappings_changed {
        let mut files = Vec::new();
        for (path, abs) in configs.iter().filter(|(p, _)| kube::is_source(p)) {
            let Ok(src) = read_up_to(abs, lang::MAX_BYTES) else {
                continue;
            };
            let big = src.len() > lang::MAX_BYTES;
            let src = if big || !kube::wanted(path, &src) {
                Vec::new()
            } else {
                src
            };
            files.push((path.clone(), src));
        }
        kube::refresh(&tx, &kube::evaluate_with(&files, &mappings))?;
    }
    let configuration_nodes = iac::nodes(&tx)?;
    let mut nodes_by_path = BTreeMap::<&str, Vec<&iac::Node>>::new();
    for node in &configuration_nodes {
        nodes_by_path.entry(&node.path).or_default().push(node);
    }
    let contract_configs = contract_found(&all, &hidden);
    let nodes_of = |path: &str| {
        nodes_by_path
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or_default()
    };
    let inputs = stored_inputs(&tx)?;
    let made = std::cell::RefCell::new(Vec::new());
    refresh_stamped(
        &tx,
        "contract_configs",
        &contract_configs,
        now,
        |path| {
            Ok(inputs_moved(
                root,
                inputs.get(path),
                nodes_of(path),
                &helm_env,
            ))
        },
        |abs| read_up_to(abs, lang::MAX_BYTES),
        |path, src| {
            SUMMARIES.set(SUMMARIES.get() + 1);
            let nodes = nodes_of(path);
            let (summary, seen) =
                contract::probed(|| contract::config_summary(path, src, root, nodes, &helm_env));
            let read = Inputs {
                nodes: nodes_digest(nodes),
                probes: seen.probes,
                reads: seen.reads,
                helm_env: helm_env.clone(),
            };
            made.borrow_mut()
                .push((path.to_string(), serde_json::to_string(&read)?));
            Ok(serde_json::to_string(&summary)?)
        },
    )?;
    for (path, read) in made.into_inner() {
        tx.execute(
            "UPDATE contract_configs SET inputs = ?2 WHERE path = ?1",
            params![path, read],
        )?;
    }
    let manifest_rows = manifest_rows(&tx)?;
    let mut changed = Vec::new();
    for &(path, abs, pack) in &walked {
        let setting = settings[&pack](path);
        let (size, mtime, trusted) = stat(abs, now)?;
        let known = stored.get(path);
        let current = known.is_some_and(|k| lang::base(&k.setting) == setting);
        if current && known.is_some_and(|k| k.mtime.is_some() && k.mtime == mtime && k.size == size)
        {
            continue;
        }
        let mtime = mtime.filter(|_| trusted);
        let src = fs::read(abs)?;
        let hash = blake3::hash(&src).to_hex().to_string();
        if current && known.is_some_and(|k| k.hash == hash) {
            if known.is_some_and(|k| (k.mtime, k.size) != (mtime, size)) {
                tx.execute(
                    "UPDATE files SET mtime = ?2, size = ?3 WHERE path = ?1",
                    params![path, mtime, size],
                )?;
            }
            continue;
        }
        let extra = known.map_or("", |k| lang::extra(&k.setting));
        let setting = lang::setting(&setting, extra);
        let mut facts = lang::pack(pack).parse_with(path, &src, &setting)?;
        lang::hash_spans(&mut facts, &src);
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

    let clients = format!("{:?}", config.channel_clients);
    let digest = configs_digest(digest, &manifest_rows, &clients, &mapping_stamp);
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

    let changed_paths: HashSet<String> = changed.iter().map(|p| p.path.clone()).collect();
    let kept: Option<Vec<(String, Facts)>> = tx
        .prepare("SELECT path, facts FROM files")?
        .query_map([], |r| {
            let path: String = r.get(0)?;
            if changed_paths.contains(&path) {
                return Ok(None);
            }
            let facts: String = r.get(1)?;
            Ok(Some((path, serde_json::from_str(&facts).ok())))
        })?
        .filter_map(|row| row.transpose())
        .map(|row| row.map(|(path, facts)| facts.map(|f| (path, f))))
        .collect::<rusqlite::Result<Option<Vec<_>>>>()?;
    let Some(mut kept) = kept else {
        drop(tx);
        db.execute("DELETE FROM meta", [])?;
        return refresh_keeping(db, root, identity, read_serve, true);
    };
    settle(
        &tx,
        &mut changed,
        &mut kept,
        &walked,
        &packs,
        &settings,
        now,
    )?;
    let changed_paths: HashSet<String> = changed.iter().map(|p| p.path.clone()).collect();
    let parsed_under: HashMap<String, String> = (changed.iter())
        .map(|p| (p.path.clone(), p.facts.setting.clone()))
        .chain(kept.iter().map(|(p, f)| (p.clone(), f.setting.clone())))
        .collect();
    let found_configs = |pack: usize| -> Vec<(&str, &[u8])> {
        (pack_files.get(&pack).into_iter().flatten())
            .map(|(n, c)| (n.as_str(), c.as_slice()))
            .collect()
    };
    let mut raw: BTreeMap<usize, BTreeMap<&str, &Facts>> = BTreeMap::new();
    let changed_facts = changed.iter().map(|p| (p.path.as_str(), &p.facts));
    for (path, facts) in changed_facts.chain(kept.iter().map(|(p, f)| (p.as_str(), f))) {
        raw.entry(packs[path]).or_default().insert(path, facts);
    }
    let joins: BTreeMap<usize, lang::Join> = (raw.iter())
        .map(|(&pack, files)| (pack, lang::pack(pack).join(&found_configs(pack), files)))
        .collect();
    drop(raw);
    // ponytail: serializes each changed file a second time and holds every file's values until
    // here, in case a join reads one (none does since #609); drop them at parse if none ever does.
    // A drop past a project's count (`Facts::usings`) is the project's, not the file's parse:
    // only a join sees it. The files whose mark a join set or cleared are stamped into their
    // stored facts below, which `contracts` reads for `meada status`.
    // ponytail: a file still over the count is read and written back on every refresh that
    // reaches here; only the files of a project past a count pay it.
    let mut restamp: Vec<(String, Option<usize>)> = Vec::new();
    for p in &mut changed {
        p.facts = serde_json::from_str(&serde_json::to_string(&p.facts)?)?;
        joins[&packs[p.path.as_str()]](&p.path, &mut p.facts);
        if p.facts.usings.is_some() {
            restamp.push((p.path.clone(), p.facts.usings));
        }
    }
    for (path, facts) in &mut kept {
        let stored = facts.usings;
        joins[&packs[path.as_str()]](path, facts);
        if facts.usings != stored {
            restamp.push((path.clone(), facts.usings));
        }
    }
    let restamped: Vec<String> = restamp.iter().map(|(p, _)| p.clone()).collect();
    for (path, usings) in restamp {
        let stored: String = tx.query_row(
            "SELECT facts FROM files WHERE path = ?1",
            params![path],
            |r| r.get(0),
        )?;
        let mut facts: Facts = serde_json::from_str(&stored)?;
        facts.usings = usings;
        tx.execute(
            "UPDATE files SET facts = ?2 WHERE path = ?1",
            params![path, serde_json::to_string(&facts)?],
        )?;
    }
    let parsed: BTreeMap<&str, &Facts> = changed
        .iter()
        .map(|p| (p.path.as_str(), &p.facts))
        .chain(kept.iter().map(|(path, facts)| (path.as_str(), facts)))
        .collect();

    let mut groups: BTreeMap<usize, BTreeMap<&str, &Facts>> = BTreeMap::new();
    for (&path, &facts) in &parsed {
        let group = lang::resolver_of(packs[path]);
        groups.entry(group).or_default().insert(path, facts);
    }
    let (manifests, tsconfigs) = read_manifests(manifest_rows)?;
    let ts_packages: BTreeSet<String> = manifests
        .iter()
        .filter(|m| m.path.rsplit('/').next() == Some("package.json"))
        .filter_map(|m| m.summary.name.clone())
        .collect();
    let mut layouts = BTreeMap::new();
    for (&pack, files) in &mut groups {
        let layout = lang::pack(pack).layout(&found_configs(pack), files)?;
        if let Some(keep) = &layout.members {
            files.retain(|path, _| keep.contains(*path));
        }
        layouts.insert(pack, layout);
    }
    let all: BTreeMap<&str, &Facts> = groups.values().flatten().map(|(p, f)| (*p, *f)).collect();
    let mut graph_moved = false;
    for path in parsed.keys().filter(|p| !all.contains_key(*p)) {
        if stored.get(*path).is_some_and(|s| s.resolved.is_empty()) {
            continue;
        }
        graph_moved = true;
        forget_calls(&tx, path)?;
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        tx.execute(
            "UPDATE files SET resolved = '' WHERE path = ?1",
            params![path],
        )?;
    }
    let mut roots: Vec<(String, String)> = Vec::new();
    for layout in layouts.values_mut() {
        roots.append(&mut layout.programs);
    }
    let group_of = |path: &str| {
        let pack = packs.get(path).copied().or_else(|| lang::pack_for(path));
        pack.map(lang::resolver_of)
    };
    let candidates = match keep && known_configs.as_deref() == Some(&digest) {
        true => candidate_groups(
            &stored,
            &groups,
            group_of,
            (changed_paths.iter().map(String::as_str))
                .chain(deleted.iter().map(|p| p.as_str()))
                .chain(restamped.iter().map(String::as_str)),
        ),
        false => BTreeSet::new(),
    };
    let mut resolvers: BTreeMap<usize, Resolver> = BTreeMap::new();
    for &pack in groups.keys().filter(|g| !candidates.contains(g)) {
        resolvers.insert(pack, resolver(pack, &groups, &mut layouts, &ts_packages));
    }
    let stored_exports = match candidates.is_empty() {
        true => Vec::new(),
        false => stored_exports(&tx)?,
    };
    let exports_of = |pack: usize, resolvers: &BTreeMap<usize, Resolver>| match resolvers.get(&pack)
    {
        Some(resolver) => resolver.exported(),
        None => (stored_exports.iter())
            .filter(|e| group_of(&e.path) == Some(pack))
            .cloned()
            .collect(),
    };
    let mut exported: Vec<Exported> = (groups.keys())
        .flat_map(|&pack| exports_of(pack, &resolvers))
        .collect();
    let kept_groups = match !candidates.is_empty() && stored_exports == exported {
        true => candidates,
        false => {
            for &pack in &candidates {
                resolvers.insert(pack, resolver(pack, &groups, &mut layouts, &ts_packages));
            }
            exported = resolvers.values().flat_map(Resolver::exported).collect();
            BTreeSet::new()
        }
    };
    let kept_files: HashSet<&str> = (kept_groups.iter())
        .flat_map(|g| groups[g].keys().copied())
        .collect();

    let files: Vec<&str> = all.keys().copied().collect();
    forget_rows_outside(&tx, "flexible", &kept_files)?;
    for f in resolvers.values().flat_map(Resolver::flexible) {
        tx.execute(
            "INSERT INTO flexible VALUES (?1, ?2, ?3, ?4)",
            params![f.path, f.name, f.least as i64, f.most.map(|m| m as i64)],
        )?;
    }
    let mut seen = HashSet::new();
    let pack_configs: Vec<(&str, &[u8])> = (pack_files.values().flatten())
        .filter(|(path, _)| seen.insert(path.as_str()))
        .map(|(path, src)| (path.as_str(), src.as_slice()))
        .collect();
    let symlinks: Vec<&str> = symlinks.iter().map(String::as_str).collect();
    let indexed = Indexed {
        manifests: &manifests,
        tsconfigs: &tsconfigs,
        exported: &exported,
        files: &files,
        configs: &pack_configs,
        symlinks: &symlinks,
        channel_clients: &config.channel_clients,
    };

    // ponytail: a group holding a changed file is extracted whole, since a kind reads the whole
    // pack at once (mounts across files, budgets spent in walk order); narrowing it per file
    // needs each kind to say what its contracts read (#391).
    let cache = values::Values::beside(db);
    let reparse = |path: &str| -> Result<Facts> {
        let src = fs::read(root.join(path))?;
        let pack = packs[path];
        let setting = parsed_under.get(path).cloned();
        let setting = setting.unwrap_or_else(|| settings[&pack](path));
        let key = values::key(pack, &setting, &src);
        // ponytail: a changed file is parsed here a second time, its first parse being under a
        // setting `settle` may still change; only the changed files of a refresh pay it.
        let mut facts = match cache.get(path, &key) {
            Some(facts) => facts,
            None => {
                VALUE_PARSES.set(VALUE_PARSES.get() + 1);
                let facts = lang::pack(pack).parse_with(path, &src, &setting)?;
                cache.put(path, key, &facts);
                facts
            }
        };
        if let Some(join) = joins.get(&pack) {
            join(path, &mut facts);
        }
        Ok(facts)
    };
    let valued = tx
        .prepare("SELECT path FROM valued")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<BTreeSet<String>>>()?;
    let mut reparsed: BTreeMap<String, Facts> = BTreeMap::new();
    for path in &valued {
        if packs.contains_key(path.as_str()) && !kept_files.contains(path.as_str()) {
            reparsed.insert(path.clone(), reparse(path)?);
        }
    }
    let mut done: BTreeMap<usize, Vec<Found>> = BTreeMap::new();
    let mut read: BTreeSet<String> = BTreeSet::new();
    let found = loop {
        let full: BTreeMap<&str, &Facts> = reparsed
            .iter()
            .map(|(path, facts)| (path.as_str(), facts))
            .collect();
        let mut misses = Vec::new();
        for (&pack, resolver) in &resolvers {
            if done.contains_key(&pack) || kept_groups.contains(&pack) {
                continue;
            }
            let code = Code::new(resolver, &full, lang::pack(pack));
            let mut found = Vec::new();
            for kind in contract::KINDS {
                found.extend(kind.extract(&code, &indexed));
            }
            debug_assert!(
                (code.read().into_iter().chain(code.misses()))
                    .all(|p| groups[&pack].contains_key(p)),
                "a group's extraction read another group's file"
            );
            debug_assert!(
                found
                    .iter()
                    .all(|f| groups[&pack].contains_key(f.site.path())),
                "a group's contract sits in another group's file"
            );
            match code.misses() {
                missed if missed.is_empty() => {
                    read.extend(code.read().into_iter().map(String::from));
                    done.insert(pack, found);
                }
                missed => misses.extend(missed.into_iter().map(String::from)),
            }
        }
        if misses.is_empty() {
            break done.into_values().flatten().collect::<Vec<_>>();
        }
        misses.sort();
        misses.dedup();
        for path in misses {
            let facts = reparse(&path)?;
            reparsed.insert(path, facts);
        }
    };
    let in_kept = |path: &&String| kept_files.contains(path.as_str());
    read.extend(valued.iter().filter(in_kept).cloned());
    if read != valued {
        tx.execute("DELETE FROM valued", [])?;
        for path in &read {
            tx.execute("INSERT INTO valued VALUES (?1)", params![path])?;
        }
    }
    cache.keep(&read);
    drop(reparsed);
    let unknown = kept_files.iter().map(|p| p.to_string()).collect();
    let names = Names::new(&found).without(unknown);

    // ponytail: a group holding a changed file resolves whole; a reverse-import graph can narrow
    // it if resolution shows up in profiles.
    // Whether the graph communities are read from moved: a deleted file, one its pack leaves
    // out, or another version or config, else a file whose symbols or call edges differ.
    graph_moved = graph_moved || !deleted.is_empty() || known_configs.as_deref() != Some(&digest);
    let (mut symbols, mut called) = stored_reach(&tx, &kept_files)?;
    let mut order: Vec<(&&str, &&Facts)> = (all.iter())
        .filter(|(path, _)| !kept_files.contains(**path))
        .collect();
    if REVERSED_WALK.get() {
        order.reverse();
    }
    for (&path, &facts) in order {
        let resolver = &resolvers[&lang::resolver_of(packs[path])];
        let rows = resolve_file(identity, path, facts, resolver, &names);
        symbols.extend(rows.symbols.iter().map(|(id, ..)| id.clone()));
        for call in rows.calls.iter().filter(|c| c.kind.is_none()) {
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
        graph_moved = graph_moved || stored_graph(&tx, path)? != graph(&rows);
        forget_calls(&tx, path)?;
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path])?;
        for (id, start, end, hash) in &rows.symbols {
            tx.execute(
                "INSERT INTO symbols VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, path, start, end, hash],
            )?;
        }
        for row in &rows.calls {
            tx.execute(
                "INSERT INTO calls VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    row.id,
                    row.caller,
                    row.target,
                    row.level.map(Level::name),
                    path,
                    row.start,
                    row.end,
                    row.narrowed,
                    row.decorates,
                    row.kind,
                    row.holder
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

    let mut providers: Vec<entry::Side> = Vec::new();
    let mut consumers: Vec<entry::Side> = Vec::new();
    let stored_contracts: Vec<(String, String, Option<String>, String)> = tx
        .prepare("SELECT path, role, contract, site FROM contracts")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut dropped: BTreeSet<&str> = BTreeSet::new();
    for (path, role, contract, site) in &stored_contracts {
        if !kept_files.contains(path.as_str()) {
            dropped.insert(path);
            continue;
        }
        let side = |contract: &String| entry::Side {
            site: site.clone(),
            contract: contract.clone(),
        };
        match (role.as_str(), contract) {
            ("provider", Some(contract)) => providers.push(side(contract)),
            ("consumer", Some(contract)) => consumers.push(side(contract)),
            _ => {}
        }
    }
    match kept_files.is_empty() {
        true => tx.execute("DELETE FROM contracts", []).map(|_| ())?,
        false => {
            for path in dropped {
                tx.execute("DELETE FROM contracts WHERE path = ?1", params![path])?;
            }
        }
    }
    let mut calls: HashMap<&str, Vec<(String, String)>> = HashMap::new();
    let mut references: HashMap<&str, Vec<(String, String)>> = HashMap::new();
    let mut rows: BTreeMap<_, Option<Shape>> = BTreeMap::new();
    for f in &found {
        let (path, site) = match &f.site {
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
            Site::Reference { path, index } => {
                let ids = (references.entry(path))
                    .or_insert_with(|| reference_ids(identity, path, all[path.as_str()], &names));
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
        let identity = f.identity.as_ref().map(serde_json::to_string).transpose()?;
        let lead = f.lead.as_ref().map(serde_json::to_string).transpose()?;
        let key = (
            role,
            site,
            &f.contract,
            &f.via,
            path,
            &f.method,
            fragments.transpose()?,
            identity,
            lead,
            f.cap.map(crate::contract::Level::name),
            &f.client,
        );
        let shape = rows.entry(key).or_default();
        match (shape.as_mut(), &f.shape) {
            (Some(shape), Some(more)) => shape.merge(more),
            (None, more) => *shape = more.clone(),
            (_, None) => {}
        }
    }
    for (key, shape) in rows {
        let (role, site, contract, via, path, method, fragments, identity, lead, cap, client) = key;
        let shape = shape.as_ref().map(serde_json::to_string).transpose()?;
        tx.execute(
            "INSERT INTO contracts VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                role, contract, site, via, path, method, fragments, shape, identity, lead, cap,
                client
            ],
        )?;
    }
    tx.execute("DELETE FROM exports", [])?;
    for e in &exported {
        tx.execute(
            "INSERT INTO exports VALUES (?1, ?2, ?3, ?4, ?5)",
            params![e.path, e.name, e.file, e.target, e.source],
        )?;
    }
    forget_rows_outside(&tx, "families", &kept_files)?;
    for a in resolvers.values().flat_map(Resolver::ancestries) {
        let ancestors = a
            .ancestors
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        tx.execute(
            "INSERT INTO families VALUES (?1, ?2, ?3)",
            params![a.path, a.class, ancestors],
        )?;
    }
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
    let doors = entry::find(&local);
    if names.missed() {
        drop(tx);
        return refresh_keeping(db, root, identity, read_serve, false);
    }
    tx.execute("DELETE FROM entry_points", [])?;
    for e in doors {
        tx.execute(
            "INSERT INTO entry_points VALUES (?1, ?2, ?3, ?4)",
            params![e.identity, e.kind.name(), e.label, e.site],
        )?;
    }
    if graph_moved {
        write_communities(&tx)?;
    }
    tx.execute("DELETE FROM meta", [])?;
    tx.execute(
        "INSERT INTO meta VALUES (?1, ?2)",
        params![identity, digest],
    )?;
    tx.commit()?;
    Ok(())
}

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

pub(crate) fn facts_loading(db: &Connection, names: &[&str]) -> Result<BTreeMap<String, Facts>> {
    let names: HashSet<&str> = names.iter().copied().collect();
    let mut out = BTreeMap::new();
    let mut rows = db.prepare("SELECT path, facts FROM files")?;
    let found = rows.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in found {
        let (path, facts) = row?;
        let names_one = (facts.split('"'))
            .any(|s| crate::package::npm::split(s).is_some_and(|(n, _)| names.contains(n)));
        if !names_one {
            continue;
        }
        if let Ok(facts) = serde_json::from_str::<Facts>(&facts) {
            out.insert(path, facts);
        }
    }
    Ok(out)
}

pub(crate) fn facts_of(db: &Connection, paths: &[&str]) -> Result<BTreeMap<String, Facts>> {
    let mut out = BTreeMap::new();
    let mut rows = db.prepare("SELECT facts FROM files WHERE path = ?1")?;
    for path in paths {
        let facts: Option<String> = rows.query_row(params![path], |r| r.get(0)).optional()?;
        if let Some(facts) = facts.and_then(|f| serde_json::from_str::<Facts>(&f).ok()) {
            out.insert(path.to_string(), facts);
        }
    }
    Ok(out)
}

fn manifest_rows(db: &Connection) -> Result<Vec<(String, String)>> {
    let rows = db
        .prepare("SELECT path, summary FROM manifests ORDER BY path")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

struct Packs<'a> {
    walked: Vec<(&'a String, &'a PathBuf, usize)>,
    packs: BTreeMap<&'a str, usize>,
    configs: BTreeMap<usize, Vec<(String, Vec<u8>)>>,
    symlinks: BTreeSet<String>,
    digest: blake3::Hasher,
    settings: BTreeMap<usize, Setting>,
}

type Setting = Box<dyn Fn(&str) -> String>;

fn packs_of<'a>(root: &Path, all: &'a [(String, PathBuf)]) -> Packs<'a> {
    let walked: Vec<(&String, &PathBuf, usize)> = (all.iter())
        .filter_map(|(path, abs)| Some((path, abs, lang::pack_for(path)?)))
        .collect();
    let packs: BTreeMap<&str, usize> = walked.iter().map(|(p, _, k)| (p.as_str(), *k)).collect();
    let mut configs: BTreeMap<usize, Vec<(String, Vec<u8>)>> =
        packs.values().map(|&pack| (pack, Vec::new())).collect();
    let mut digest = blake3::Hasher::new();
    let seen: HashSet<&str> = all.iter().map(|(path, _)| path.as_str()).collect();
    let mut symlinks = BTreeSet::new();
    for (&pack, found) in &mut configs {
        for &name in lang::pack(pack).config_files() {
            let paths: BTreeSet<&str> = match name.strip_prefix("**/") {
                Some(file) => (all.iter())
                    .map(|(path, _)| path.as_str())
                    .filter(|path| {
                        lang::names_config(file, path.rsplit('/').next().unwrap_or(path))
                    })
                    .collect(),
                None => BTreeSet::from([name]),
            };
            for path in paths {
                if let Some(src) = read_config(root, path) {
                    digest.update(format!("{pack}\0{path}\0{}\0", src.len()).as_bytes());
                    digest.update(&src);
                    found.push((path.to_string(), src));
                }
            }
        }
        let inputs = config_inputs(root, pack, found, &seen);
        for (path, input) in inputs {
            match input {
                lang::ConfigInput::Read(src) => {
                    digest.update(format!("{pack}\0{path}\0{}\0", src.len()).as_bytes());
                    digest.update(&src);
                    found.push((path, src.into_owned()));
                }
                lang::ConfigInput::Symlink => {
                    digest.update(format!("{pack}\0{path}\0symlink\0").as_bytes());
                    symlinks.insert(path);
                }
                lang::ConfigInput::Missing => {}
            }
        }
    }
    let settings = (configs.iter())
        .map(|(&pack, found)| {
            let found: Vec<(&str, &[u8])> = found
                .iter()
                .map(|(n, c)| (n.as_str(), c.as_slice()))
                .collect();
            (pack, lang::pack(pack).settings(&found))
        })
        .collect();
    Packs {
        walked,
        packs,
        configs,
        symlinks,
        digest,
        settings,
    }
}

fn config_inputs(
    root: &Path,
    pack: usize,
    found: &[(String, Vec<u8>)],
    seen: &HashSet<&str>,
) -> BTreeMap<String, lang::ConfigInput<'static>> {
    let configs: Vec<(&str, &[u8])> = found
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_slice()))
        .collect();
    let inputs = std::cell::RefCell::new(BTreeMap::new());
    let read = |path: &str| {
        let input = match seen
            .contains(path)
            .then(|| read_config(root, path))
            .flatten()
        {
            Some(src) => lang::ConfigInput::Read(src.into()),
            None if plain_file(root, path).is_none() && through_link(root, path) => {
                lang::ConfigInput::Symlink
            }
            None => lang::ConfigInput::Missing,
        };
        inputs.borrow_mut().insert(path.to_string(), input.clone());
        input
    };
    lang::pack(pack).config_inputs(&configs, &read);
    let mut inputs = inputs.into_inner();
    inputs.retain(|path, _| !configs.iter().any(|(p, _)| p == path));
    inputs
}

fn through_link(root: &Path, path: &str) -> bool {
    let mut abs = root.to_path_buf();
    for segment in path.split('/') {
        abs.push(segment);
        match fs::symlink_metadata(&abs) {
            Ok(meta) if meta.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

fn configs_digest(
    mut digest: blake3::Hasher,
    manifest_rows: &[(String, String)],
    clients: &str,
    mappings: &str,
) -> String {
    for (path, summary) in manifest_rows {
        digest.update(format!("{path}\0{summary}\0").as_bytes());
    }
    digest.update(clients.as_bytes());
    digest.update(env!("CARGO_PKG_VERSION").as_bytes());
    format!("{}:{mappings}", digest.finalize().to_hex())
}

fn candidate_groups<'p>(
    stored: &HashMap<String, Stored>,
    groups: &BTreeMap<usize, BTreeMap<&str, &Facts>>,
    group_of: impl Fn(&str) -> Option<usize>,
    touched: impl Iterator<Item = &'p str>,
) -> BTreeSet<usize> {
    let before = (stored.iter()).filter(|(_, s)| !s.resolved.is_empty());
    let before: BTreeSet<&str> = before.map(|(path, _)| path.as_str()).collect();
    let now: BTreeSet<&str> = groups.values().flat_map(|f| f.keys().copied()).collect();
    if before != now {
        return BTreeSet::new();
    }
    let touched: BTreeSet<usize> = touched.filter_map(group_of).collect();
    (groups.keys().copied())
        .filter(|g| !touched.contains(g))
        .collect()
}

fn resolver<'a>(
    pack: usize,
    groups: &'a BTreeMap<usize, BTreeMap<&'a str, &'a Facts>>,
    layouts: &mut BTreeMap<usize, lang::Layout>,
    packages: &BTreeSet<String>,
) -> Resolver<'a> {
    let layout = layouts.remove(&pack).expect("a layout per pack");
    let language = lang::pack(pack);
    if language.extensions().contains(&"ts") {
        Resolver::with_layout_and_packages(&groups[&pack], layout, language, packages.clone())
    } else {
        Resolver::with_layout(&groups[&pack], layout, language)
    }
}

fn stored_exports(db: &Connection) -> Result<Vec<Exported>> {
    let mut statement =
        db.prepare("SELECT path, name, file, target, source FROM exports ORDER BY rowid")?;
    let rows = statement.query_map([], |r| {
        Ok(Exported {
            path: r.get(0)?,
            name: r.get(1)?,
            file: r.get(2)?,
            target: r.get(3)?,
            source: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn exports_of(db: &Connection, paths: &[String]) -> Result<Vec<Exported>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    Ok(db
        .prepare(
            "SELECT path, name, file, target, source FROM exports WHERE path IN \
         (SELECT value FROM json_each(?1)) ORDER BY path, name, file, target",
        )?
        .query_map(params![serde_json::to_string(paths)?], |r| {
            Ok(Exported {
                path: r.get(0)?,
                name: r.get(1)?,
                file: r.get(2)?,
                target: r.get(3)?,
                source: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn forget_rows_outside(tx: &Transaction, table: &str, kept: &HashSet<&str>) -> Result<()> {
    if kept.is_empty() {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
        return Ok(());
    }
    let paths: Vec<String> = (tx.prepare(&format!("SELECT DISTINCT path FROM {table}"))?)
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for path in paths.iter().filter(|p| !kept.contains(p.as_str())) {
        tx.execute(
            &format!("DELETE FROM {table} WHERE path = ?1"),
            params![path],
        )?;
    }
    Ok(())
}

fn stored_reach(
    db: &Connection,
    kept: &HashSet<&str>,
) -> Result<(HashSet<String>, HashSet<String>)> {
    let (mut symbols, mut called) = (HashSet::new(), HashSet::new());
    if kept.is_empty() {
        return Ok((symbols, called));
    }
    let mut statement = db.prepare("SELECT path, id FROM symbols")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?;
    for row in rows {
        let (path, id) = row?;
        if kept.contains(path.as_str()) {
            symbols.insert(id);
        }
    }
    for query in [
        "SELECT path, caller, target FROM calls WHERE target IS NOT NULL AND kind IS NULL",
        "SELECT calls.path, calls.caller, candidates.target FROM candidates \
         JOIN calls ON calls.id = candidates.call WHERE calls.kind IS NULL",
    ] {
        let mut statement = db.prepare(query)?;
        let rows = statement.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?))
        })?;
        for row in rows {
            let (path, caller, target): (String, String, String) = row?;
            if kept.contains(path.as_str()) && caller != target {
                called.insert(target);
            }
        }
    }
    Ok((symbols, called))
}

pub fn fresh(db: &Connection, root: &Path, identity: &str, serve: &Serve) -> Result<bool> {
    let now = SystemTime::now();
    let meta = (db.query_row("SELECT identity, configs FROM meta", [], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }))
    .optional()?;
    let Some((_, configs)) = meta.filter(|(known, _)| known == identity) else {
        return Ok(false);
    };
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
    let hidden = hidden_ci(root);
    let (config_files, shadowed) = configs_found(&all, &hidden);
    let tables = [
        (
            "documents",
            documents_found(root, &all, serve),
            document::DOCUMENT_BYTES_MAX,
        ),
        ("manifests", manifests_found(&all), lang::MAX_BYTES),
        ("lockfiles", lockfiles_found(&all), lock::MAX_LOCK_BYTES),
        ("config_files", config_files.clone(), lang::MAX_BYTES),
        (
            "contract_configs",
            contract_found(&all, &hidden),
            lang::MAX_BYTES,
        ),
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
                None if stat(abs, now).is_err() || read(abs).is_err() => {}
                None => return Ok(false),
            }
        }
        if seen != stored.len() {
            return Ok(false);
        }
    }
    let nodes = iac::nodes(db)?;
    let mut nodes_by_path = BTreeMap::<&str, Vec<&iac::Node>>::new();
    for node in &nodes {
        nodes_by_path.entry(&node.path).or_default().push(node);
    }
    let contract_configs = contract_found(&all, &hidden);
    let inputs = stored_inputs(db)?;
    let config = crate::config::load(root)?;
    let helm_env: Vec<String> = config.helm_env.iter().map(|h| h.key.clone()).collect();
    for (path, _) in &contract_configs {
        let nodes = nodes_by_path.get(path.as_str()).map(Vec::as_slice);
        if inputs.contains_key(path)
            && inputs_moved(root, inputs.get(path), nodes.unwrap_or_default(), &helm_env)
        {
            return Ok(false);
        }
    }
    let sensitive = config_values::Sensitive::new(db, &config_files, &shadowed);
    for (path, _) in &config_files {
        if sensitive.merged(path)?.is_some() || sensitive.stale(path)? {
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
        let base = lang::base(&k.setting);
        if base != settings[&pack](path) || !same(&k.hash, k.mtime, k.size, abs, &read) {
            return Ok(false);
        }
    }
    if stored.keys().any(|p| !packs.contains_key(p.as_str())) {
        return Ok(false);
    }
    let mappings = config.iac.stamp()?;
    let clients = format!("{:?}", config.channel_clients);
    Ok(configs_digest(digest, &manifest_rows(db)?, &clients, &mappings) == configs)
}

type Graph = (Vec<String>, Vec<(String, String)>);

fn stored_graph(tx: &Transaction, path: &str) -> Result<Graph> {
    let mut symbols: Vec<String> = (tx.prepare_cached("SELECT id FROM symbols WHERE path = ?1")?)
        .query_map(params![path], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut calls: Vec<(String, String)> = (tx.prepare_cached(
        "SELECT COALESCE(holder, caller), target FROM calls WHERE path = ?1 \
         AND level IN ('exact', 'inferred') AND target IS NOT NULL",
    )?)
    .query_map(params![path], |r| Ok((r.get(0)?, r.get(1)?)))?
    .collect::<rusqlite::Result<_>>()?;
    symbols.sort();
    calls.sort();
    Ok((symbols, calls))
}

fn graph(rows: &Rows) -> Graph {
    let mut symbols: Vec<String> = rows.symbols.iter().map(|(id, ..)| id.clone()).collect();
    let mut calls: Vec<(String, String)> = (rows.calls.iter())
        .filter(|c| matches!(c.level, Some(Level::Exact | Level::Inferred)))
        .filter_map(|c| {
            Some((
                c.holder.as_ref().unwrap_or(&c.caller).clone(),
                c.target.clone()?,
            ))
        })
        .collect();
    symbols.sort();
    calls.sort();
    (symbols, calls)
}

fn write_communities(tx: &Transaction) -> Result<()> {
    let dirs: HashMap<String, String> = tx
        .prepare("SELECT id, path FROM symbols")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let calls: Vec<(String, String)> = tx
        .prepare(
            "SELECT COALESCE(holder, caller), target FROM calls \
             WHERE level IN ('exact', 'inferred') AND target IS NOT NULL",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
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

pub fn forget(db: &Connection, path: &str) -> Result<()> {
    db.execute(
        "UPDATE files SET mtime = NULL WHERE path = ?1",
        params![path],
    )?;
    Ok(())
}

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

pub(crate) fn stored_setting(db: &Connection, path: &str) -> Option<String> {
    (db.query_row(
        "SELECT setting FROM files WHERE path = ?1",
        params![path],
        |r| r.get(0),
    ))
    .ok()
}

pub(crate) fn read_config(root: &Path, path: &str) -> Option<Vec<u8>> {
    read_up_to(&plain_file(root, path)?, lang::MAX_BYTES).ok()
}

fn read_up_to(abs: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    let mut src = Vec::new();
    fs::File::open(abs)?
        .take(max as u64 + 1)
        .read_to_end(&mut src)?;
    Ok(src)
}

fn manifests_found(walked: &[(String, PathBuf)]) -> Vec<(String, PathBuf)> {
    let is_manifest =
        |path: &str| package::is_manifest(path) && !path.split('/').any(|s| s == "node_modules");
    (walked.iter())
        .filter(|(path, _)| is_manifest(path))
        .cloned()
        .collect()
}

fn lockfiles_found(walked: &[(String, PathBuf)]) -> Vec<(String, PathBuf)> {
    let is_lockfile = |path: &str| {
        path.rsplit('/').next() == Some("yarn.lock")
            && !path.split('/').any(|s| s == "node_modules")
    };
    (walked.iter())
        .filter(|(path, _)| is_lockfile(path))
        .cloned()
        .collect()
}

fn resolve_lockfile(tx: &Transaction, root: &Path, path: &str, src: &[u8]) -> Result<String> {
    tx.execute("DELETE FROM resolved WHERE lockfile = ?1", params![path])?;
    let folder = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let manifest = repo::relative(folder, "package.json").unwrap_or_default();
    let overrides = (repo::regular(&root.join(&manifest), lang::MAX_BYTES))
        .filter(|b| b.len() <= lang::MAX_BYTES)
        .map_or_else(BTreeMap::new, |b| lock::overrides(&b));
    let lock = match lock::read(src, &overrides) {
        Ok(lock) => lock,
        Err(unread) => return Ok(unread.name().to_string()),
    };
    for r in lock::direct(&lock) {
        let Some(member) = repo::relative(folder, &format!("{}/package.json", r.dir))
            .filter(|m| m != ".." && !m.starts_with("../"))
        else {
            continue;
        };
        tx.execute(
            "INSERT INTO resolved (lockfile, manifest, name, origin, version) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![path, member, r.name, r.origin, r.version],
        )?;
    }
    Ok("read".to_string())
}

fn resolved_rows(db: &Connection, manifests: &[Manifest]) -> Result<Vec<written::Resolved>> {
    let mut rows = db.prepare(
        "SELECT DISTINCT manifest, name, origin, version FROM resolved \
         ORDER BY manifest, name, origin, version",
    )?;
    let rows = rows.query_map([], |r| {
        let mut resolved = written::Resolved::default();
        resolved.manifest = r.get(0)?;
        resolved.name = r.get(1)?;
        resolved.origin = r.get(2)?;
        resolved.version = r.get(3)?;
        Ok(resolved)
    })?;
    let mut out = Vec::new();
    for row in rows {
        let mut resolved = row?;
        resolved.range = (manifests.iter())
            .find(|m| m.path == resolved.manifest)
            .and_then(|m| {
                (m.summary.dependencies.iter())
                    .find(|e| e.name == resolved.name && e.range.is_some())
            })
            .and_then(|e| e.range.clone());
        out.push(resolved);
    }
    Ok(out)
}

fn stored_dockerfiles(db: &Connection) -> Result<Vec<String>> {
    let mut rows = db.prepare("SELECT folder FROM dockerfiles ORDER BY folder")?;
    let folders = rows.query_map([], |r| r.get(0))?;
    Ok(folders.collect::<rusqlite::Result<_>>()?)
}

fn configs_found(
    walked: &[(String, PathBuf)],
    hidden: &[(String, PathBuf)],
) -> (Vec<(String, PathBuf)>, Vec<String>) {
    let mut found: Vec<_> = (walked.iter().chain(hidden))
        .filter(|(path, _)| config_values::is_config(path))
        .cloned()
        .collect();
    found.sort_by(|(a, _), (b, _)| {
        (config_values::is_tfvars(a), a).cmp(&(config_values::is_tfvars(b), b))
    });
    config_values::unshadowed(found)
}

fn hidden_ci(root: &Path) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = WalkBuilder::new(root)
        .require_git(false)
        .parents(false)
        .hidden(false)
        .add_custom_ignore_filename(".meadaignore")
        .filter_entry(|e| match e.depth() {
            1 => (e.file_name().to_str()).is_some_and(config_values::is_hidden_ci),
            _ => !is_build_output(e.path()) && !is_default_skip(e.path()),
        })
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|entry| Some((relative(root, entry.path())?, entry.into_path())))
        .collect();
    files.sort();
    files
}

fn refresh_stamped(
    tx: &Transaction,
    table: &str,
    found: &[(String, PathBuf)],
    now: SystemTime,
    forced: impl Fn(&str) -> Result<bool>,
    read: impl Fn(&Path) -> std::io::Result<Vec<u8>>,
    summarize: impl Fn(&str, &[u8]) -> Result<String>,
) -> Result<Vec<String>> {
    let stored: HashMap<String, Stamp> = tx
        .prepare(&format!("SELECT path, hash, mtime, size FROM {table}"))?
        .query_map([], |r| {
            let (hash, mtime, size) = (r.get(1)?, r.get(2)?, r.get(3)?);
            Ok((r.get(0)?, Stamp { hash, mtime, size }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut seen = HashSet::new();
    for (path, abs) in found {
        let Ok((size, mtime, trusted)) = stat(abs, now) else {
            continue;
        };
        seen.insert(path.as_str());
        let known = stored.get(path);
        let forced = forced(path)?;
        if !forced && known.is_some_and(|k| k.mtime.is_some() && k.mtime == mtime && k.size == size)
        {
            continue;
        }
        let mtime = mtime.filter(|_| trusted);
        let Ok(src) = read(abs) else {
            seen.remove(path.as_str());
            continue;
        };
        let hash = blake3::hash(&src).to_hex().to_string();
        if !forced && known.is_some_and(|k| k.hash == hash) {
            if known.is_some_and(|k| (k.mtime, k.size) != (mtime, size)) {
                tx.execute(
                    &format!("UPDATE {table} SET mtime = ?2, size = ?3 WHERE path = ?1"),
                    params![path, mtime, size],
                )?;
            }
            continue;
        }
        let summary = summarize(path, &src)?;
        let column = if table == "lockfiles" {
            "state"
        } else {
            "summary"
        };
        tx.execute(
            &format!(
                "INSERT OR REPLACE INTO {table} (path, hash, mtime, size, {column}) \
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![path, hash, mtime, size, summary],
        )?;
    }
    let gone: Vec<String> = (stored.into_keys())
        .filter(|p| !seen.contains(p.as_str()))
        .collect();
    for path in &gone {
        tx.execute(
            &format!("DELETE FROM {table} WHERE path = ?1"),
            params![path],
        )?;
    }
    Ok(gone)
}

fn contract_found(
    all: &[(String, PathBuf)],
    hidden: &[(String, PathBuf)],
) -> Vec<(String, PathBuf)> {
    let found = (all.iter().chain(hidden)).filter(|(p, _)| contract::config_file(p));
    config_values::unshadowed(found.cloned().collect()).0
}

#[derive(Serialize, serde::Deserialize)]
struct Inputs {
    nodes: String,
    probes: BTreeMap<String, contract::Probe>,
    #[serde(default)]
    reads: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    helm_env: Vec<String>,
}

fn stored_inputs(db: &Connection) -> Result<HashMap<String, String>> {
    Ok(db
        .prepare("SELECT path, inputs FROM contract_configs")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

fn nodes_digest(nodes: &[&iac::Node]) -> String {
    blake3::hash(format!("{nodes:?}").as_bytes())
        .to_hex()
        .to_string()
}

fn inputs_moved(
    root: &Path,
    inputs: Option<&String>,
    nodes: &[&iac::Node],
    helm_env: &[String],
) -> bool {
    let Some(Ok(read)) = inputs.map(|i| serde_json::from_str::<Inputs>(i)) else {
        return true;
    };
    read.helm_env != helm_env
        || read.nodes != nodes_digest(nodes)
        || (read.probes.iter()).any(|(rel, was)| contract::probe(root, rel) != *was)
        || (read.reads.iter()).any(|(key, was)| contract::fingerprint(root, key) != *was)
}

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
    static VALUE_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SUMMARIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many contract configurations this thread's refreshes have summarized (`contract_configs`).
/// Test-only: no command, option or setting reads it.
#[doc(hidden)]
pub fn contract_summaries() -> usize {
    SUMMARIES.get()
}

/// How many files this thread's refreshes have parsed for their values (`valued`), past the
/// values cache (ADR 0027). Test-only: no command, option or setting reads it.
#[doc(hidden)]
pub fn value_parses() -> usize {
    VALUE_PARSES.get()
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

fn is_build_output(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("obj" | "bin")
    ) && path.is_dir()
        && path
            .parent()
            .and_then(|p| fs::read_dir(p).ok())
            .is_some_and(|mut d| {
                d.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "csproj")))
            })
}

fn is_default_skip(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    match name {
        "node_modules" | "Pods" => path.is_dir(),
        "Checkouts" => {
            path.is_dir()
                && path
                    .parent()
                    .and_then(|p| p.file_name())
                    .is_some_and(|n| n == "Carthage")
        }
        _ => name.ends_with(".min.js") || name.ends_with(".min.css"),
    }
}

fn walk(root: &Path) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = WalkBuilder::new(root)
        .require_git(false)
        .parents(false)
        .add_custom_ignore_filename(".meadaignore")
        .filter_entry(|e| !is_build_output(e.path()) && !is_default_skip(e.path()))
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

fn settle(
    tx: &Transaction,
    changed: &mut Vec<Parsed>,
    kept: &mut Vec<(String, Facts)>,
    walked: &[(&String, &PathBuf, usize)],
    packs: &BTreeMap<&str, usize>,
    settings: &BTreeMap<usize, Setting>,
    now: SystemTime,
) -> Result<()> {
    let mut wanted: BTreeMap<String, String> = BTreeMap::new();
    let mut files: BTreeMap<usize, BTreeMap<&str, &Facts>> = BTreeMap::new();
    let changed_facts = changed.iter().map(|p| (p.path.as_str(), &p.facts));
    for (path, facts) in changed_facts.chain(kept.iter().map(|(p, f)| (p.as_str(), f))) {
        files.entry(packs[path]).or_default().insert(path, facts);
    }
    for (&pack, files) in &files {
        let extra = lang::pack(pack).extend(files);
        for (&path, facts) in files {
            let extra = extra.get(path).map_or("", String::as_str);
            let setting = lang::setting(&settings[&pack](path), extra);
            if facts.setting != setting {
                wanted.insert(path.to_string(), setting);
            }
        }
    }
    drop(files);
    if wanted.is_empty() {
        return Ok(());
    }
    kept.retain(|(path, _)| !wanted.contains_key(path));
    changed.retain(|p| !wanted.contains_key(&p.path));
    for &(path, abs, pack) in walked {
        let Some(setting) = wanted.get(path.as_str()) else {
            continue;
        };
        let (size, mtime, trusted) = stat(abs, now)?;
        let src = fs::read(abs)?;
        let hash = blake3::hash(&src).to_hex().to_string();
        let mut facts = lang::pack(pack).parse_with(path, &src, setting)?;
        lang::hash_spans(&mut facts, &src);
        let parsed = Parsed {
            path: path.clone(),
            hash,
            mtime: mtime.filter(|_| trusted),
            size,
            facts,
        };
        write_file(tx, &parsed)?;
        changed.push(parsed);
    }
    Ok(())
}

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

fn resolve_file<'a>(
    identity: &str,
    path: &'a str,
    facts: &'a Facts,
    resolver: &Resolver<'a>,
    names: &Names,
) -> Rows<'a> {
    let id = |t: &Target| {
        let name = names.def(t.path, t.name).unwrap_or_else(|| t.name.into());
        format!("{identity}::{}::{name}", t.path)
    };
    let symbols: Vec<_> = facts
        .defs
        .iter()
        .filter_map(|def| {
            let name = names.def(path, &def.name)?;
            let id = format!("{identity}::{path}::{name}");
            Some((
                id,
                def.range.start as i64,
                def.range.end as i64,
                def.hash.as_str(),
            ))
        })
        .chain(
            (facts.declarations.iter())
                .filter(|_| facts.budget.is_none() && facts.usings.is_none() && !facts.misparsed)
                .filter_map(|d| {
                    let name = names.def(path, &d.name)?;
                    Some((
                        format!("{identity}::{path}::{name}"),
                        d.range.start as i64,
                        d.range.end as i64,
                        d.hash.as_str(),
                    ))
                }),
        )
        .collect();
    let calls = facts
        .calls
        .iter()
        .zip(call_ids(identity, path, facts, names))
        .enumerate()
        .map(|(index, (call, (caller, call_id)))| {
            let resolution = resolver.resolved_at(path, index);
            let narrowed = matches!(resolution, Resolution::Narrowed(_));
            let (target, level, candidates) = match resolution {
                Resolution::Exact(t) => (Some(id(&t)), Some(Level::Exact), Vec::new()),
                Resolution::Inferred(t) | Resolution::Narrowed(t) => {
                    (Some(id(&t)), Some(Level::Inferred), Vec::new())
                }
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
                narrowed,
                decorates: call
                    .decorates
                    .as_deref()
                    .map(|d| format!("{identity}::{path}::{}", names.scope(path, d))),
                kind: None,
                holder: None,
            }
        })
        .collect();
    let mut calls: Vec<Resolved> = calls;
    calls.extend(reference_rows(identity, path, facts, resolver, names));
    hold(&symbols, &mut calls, &format!("{identity}::{path}"));
    Rows { symbols, calls }
}

fn reference_rows(
    identity: &str,
    path: &str,
    facts: &Facts,
    resolver: &Resolver,
    names: &Names,
) -> Vec<Resolved> {
    let ids = reference_ids(identity, path, facts, names);
    let mut rows = Vec::new();
    for ((index, r), (caller, id)) in facts.references.iter().enumerate().zip(ids) {
        let Some(target) = resolver.reference(path, index) else {
            continue;
        };
        let target = target.map(|(at, name)| {
            let name = names.def(at, name).unwrap_or_else(|| name.into());
            format!("{identity}::{at}::{name}")
        });
        rows.push(Resolved {
            id,
            caller,
            level: target.is_some().then_some(Level::Exact),
            target,
            start: r.range.start as i64,
            end: r.range.end as i64,
            candidates: Vec::new(),
            narrowed: false,
            decorates: None,
            kind: Some(r.kind.as_str()),
            holder: None,
        });
    }
    rows
}

fn reference_ids(
    identity: &str,
    path: &str,
    facts: &Facts,
    names: &Names,
) -> Vec<(String, String)> {
    let base = format!("{identity}::{path}");
    let mut ordinals: HashMap<(String, &str, &str), usize> = HashMap::new();
    (facts.references.iter())
        .map(|r| {
            let scope = names.scope(path, &r.scope);
            let caller = match scope.is_empty() {
                true => base.clone(),
                false => format!("{base}::{scope}"),
            };
            let kind = r.kind.as_str();
            let ordinal = (ordinals.entry((scope, kind, r.name.as_str())))
                .and_modify(|n| *n += 1)
                .or_insert(0);
            let id = format!("{caller}::({kind}){}#{ordinal}", r.name);
            (caller, id)
        })
        .collect()
}

fn hold(symbols: &[(String, i64, i64, &str)], calls: &mut [Resolved], file: &str) {
    let mut spans: Vec<(i64, i64, &str)> = (symbols.iter())
        .map(|(id, start, end, _)| (*start, *end, id.as_str()))
        .collect();
    spans.sort_by_key(|&(start, end, _)| (start, std::cmp::Reverse(end)));
    let mut rows: Vec<usize> = (0..calls.len())
        .filter(|&i| calls[i].caller == file)
        .collect();
    rows.sort_by_key(|&i| calls[i].start);
    let (mut next, mut open) = (0, Vec::<(i64, i64, &str)>::new());
    for i in rows {
        let (start, end) = (calls[i].start, calls[i].end);
        while let Some(&span) = spans.get(next).filter(|span| span.0 <= start) {
            while open.last().is_some_and(|o| o.1 <= span.0) {
                open.pop();
            }
            open.push(span);
            next += 1;
        }
        while open.last().is_some_and(|o| o.1 < start) {
            open.pop();
        }
        let holder = open.iter().rev().find(|o| o.1 >= end);
        calls[i].holder = holder.map(|o| o.2.to_string());
    }
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = path
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|c| c.as_os_str().to_str())
        .collect();
    Some(parts?.join("/"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dropped {
    Bytes(usize),
    Usings(usize),
    Misparsed,
}

#[cfg_attr(test, derive(Default))]
pub(crate) struct Export {
    pub(crate) repository: String,
    pub(crate) root: PathBuf,
    pub(crate) commit: Option<String>,
    pub(crate) decisions: crate::decisions::Decisions,
    pub(crate) name_candidates: std::collections::BTreeMap<String, crate::items::NameCandidates>,
    pub(crate) files: Vec<File>,
    pub(crate) dropped: Vec<(String, Dropped)>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) calls: Vec<CallSite>,
    pub(crate) providers: Vec<Provider>,
    pub(crate) consumers: Vec<Consumer>,
    /// The RabbitMQ bindings the repository declares (spec #20), neither side of a channel.
    pub(crate) bindings: Vec<Binding>,
    pub(crate) provision_uses: Vec<crate::contract::ProvisionUse>,
    pub(crate) documents: Vec<Document>,
    /// The RabbitMQ publishes to a named exchange (spec #20), producers of the queues the
    /// bindings route them to.
    pub(crate) publishes: Vec<Publish>,
    pub(crate) manifests: Vec<Manifest>,
    pub(crate) tsconfigs: Vec<Config>,
    /// The packages the repository provides, by name, then directory.
    pub(crate) packages: Vec<Package>,
    /// The dependency entries naming a package of the workspace, by manifest, field, name.
    pub(crate) dependencies: Vec<Dependency>,
    pub(crate) resolved: Vec<written::Resolved>,
    pub(crate) import_calls: Vec<ImportCall>,
    pub(crate) exported: Vec<Exported>,
    pub(crate) ancestries: Vec<Ancestry>,
    pub(crate) flexible: Vec<Flexible>,
    pub(crate) shapes: Vec<package::ExportedShape>,
    /// The imports resolved into a package of the workspace, by call site, then target.
    pub(crate) imports: Vec<Import>,
    /// The repository's communities (spec #31), by name, each with its sorted members.
    pub(crate) communities: Vec<Community>,
    pub(crate) links: Vec<Link>,
    pub(crate) provisioners: Vec<iac::provision::Side>,
    pub(crate) config_uses: Vec<iac::config_uses::Side>,
    pub(crate) scripts: Vec<iac::actions::run::Script>,
    pub(crate) policy_grants: Vec<iac::config_uses::Side>,
    pub(crate) config_sites: Vec<config_values::sites::Site>,
    pub(crate) configurations: Vec<(String, serde_json::Value)>,
    pub(crate) iac: Vec<iac::Node>,
    pub(crate) iac_references: Vec<iac::Reference>,
    pub(crate) dockerfiles: Vec<String>,
    pub(crate) http: Option<crate::http::Prepared>,
    pub(crate) workload_images: Vec<(String, String, String)>,
    pub(crate) executions: Vec<written::Execution>,
    pub(crate) subscriptions: Vec<iac::subscription::Subscription>,
    pub(crate) provisions: Vec<written::Provision>,
    pub(crate) entry_points: Vec<EntryPoint>,
    pub(crate) processes: Vec<Process>,
    pub(crate) stale_decisions: Vec<Stale>,
    pub(crate) unmounted: Vec<String>,
    pub(crate) unfollowed: Vec<(String, Option<String>, Option<String>)>,
}

pub(crate) fn sort_providers(providers: &mut [Provider]) {
    providers.sort_by(|a, b| {
        (&a.contract, &a.site, &a.identity).cmp(&(&b.contract, &b.site, &b.identity))
    });
}

pub(crate) fn sort_consumers(consumers: &mut [Consumer]) {
    consumers.sort_by(|a, b| {
        (&a.site, &a.contract, &a.via, &a.root, &a.identity).cmp(&(
            &b.site,
            &b.contract,
            &b.via,
            &b.root,
            &b.identity,
        ))
    });
}

impl Export {
    pub(crate) fn written(self) -> written::Export {
        let mut out = written::Export::default();
        out.repository = self.repository;
        out.files = self.files;
        out.symbols = self.symbols;
        out.calls = self.calls;
        out.providers = (self.providers.into_iter())
            .map(|p| {
                let mut provider = written::Provider::default();
                provider.contract = p.contract;
                provider.site = p.site;
                provider.via = p.via;
                provider.at = p.at;
                provider.gateway = p.gateway;
                provider.shape = p.shape;
                provider
            })
            .collect();
        out.consumers = (self.consumers.into_iter())
            .map(|c| {
                let mut consumer = written::Consumer::default();
                consumer.contract = c.contract;
                consumer.site = c.site;
                consumer.via = c.via;
                consumer.shape = c.shape;
                consumer.root = c.root;
                consumer
            })
            .collect();
        out.bindings = self.bindings;
        out.documents = (self.documents.into_iter())
            .filter_map(|d| {
                let contracts = (d.summary.routes.iter().map(|r| r.contract()))
                    .chain(d.summary.channels.iter().map(|s| s.contract()))
                    .collect();
                let format = d.summary.format?;
                let (path, version) = (d.path, d.summary.version);
                Some(written::Document::new(path, format, version, contracts))
            })
            .collect();
        out.publishes = self.publishes;
        out.packages = self.packages.into_iter().map(Package::written).collect();
        out.dependencies = self.dependencies;
        out.resolved = self.resolved;
        out.imports = self.imports;
        out.communities = self.communities;
        out.links = self.links;
        for side in self.provisioners {
            let (node, provisioner) = side.written();
            out.iac_nodes.push(node);
            out.provisioners.push(provisioner);
        }
        for node in &self.iac {
            let identity = node.identity(&out.repository);
            if out
                .consumers
                .iter()
                .any(|c| c.site == identity && c.root.as_deref().is_none_or(|r| r == node.root))
                || out.providers.iter().any(|p| p.site == identity)
                || out.bindings.iter().any(|b| b.site == identity)
                || self.executions.iter().any(|x| x.workload == identity)
                || (self.config_uses.iter()).any(|u| u.holder.as_deref() == Some(&identity))
            {
                let mut written = written::IacNode::default();
                written.identity = identity;
                written.address = node.address.clone();
                written.ty = node.ty.clone();
                written.root = node.root.clone();
                written.path = node.path.clone();
                written.line = node.line;
                written.end_line = node.end_line;
                out.iac_nodes.push(written);
            }
        }
        out.iac_nodes
            .sort_by(|a, b| (&a.identity, &a.root).cmp(&(&b.identity, &b.root)));
        out.iac_nodes.dedup();
        out.provisions = self
            .provisions
            .into_iter()
            .map(|mut link| {
                let key = link
                    .at
                    .as_ref()
                    .and_then(|at| link.side.strip_suffix(at.as_str()));
                if let Some(repository) = key.and_then(|side| side.strip_suffix("::")) {
                    link.side = repository.to_string();
                }
                link
            })
            .collect();
        out.provisions.sort();
        out.provisions.dedup();
        out.uses = self
            .config_uses
            .iter()
            .map(|u| u.written(&out.repository))
            .collect();
        out.policy_grants = self
            .policy_grants
            .iter()
            .map(|g| g.granted(&out.repository))
            .collect();
        out.executions = self.executions;
        out.subscriptions = self
            .subscriptions
            .into_iter()
            .map(|s| {
                let mut summary = written::Subscription::default();
                summary.source = s.source;
                summary.target = s.target;
                summary.node = s.node;
                summary.root = s.root;
                summary
            })
            .collect();
        out.subscriptions.sort();
        out.subscriptions.dedup();
        out.processes = self.processes;
        out.stale_decisions = self.stale_decisions;
        for kind in crate::contract::KINDS {
            kind.written(&self.iac, &mut out);
        }
        out
    }

    pub(crate) fn place(&self) -> (&str, &Path) {
        (&self.repository, &self.root)
    }

    pub(crate) fn document_of(&self, site: &str) -> Option<&str> {
        let rest = site.strip_prefix(&self.repository)?.strip_prefix("::")?;
        let (path, _) = rest.split_once("::")?;
        let doc = self.documents.iter().find(|d| d.path == path)?;
        Some(&doc.path)
    }
}

pub(crate) struct Document {
    pub(crate) path: String,
    pub(crate) summary: Summary,
}

/// The provider side of a contract: a symbol that serves it, a file, a repository, the call
/// that produces to a channel, or a route or channel side only a document lists
/// (`<repository>::<document>::<METHOD> <path>`, or `... <broker> send <name>`).
#[derive(Clone, Default)]
pub(crate) struct Provider {
    pub(crate) contract: String,
    pub(crate) site: String,
    pub(crate) via: Option<String>,
    pub(crate) method: Option<String>,
    pub(crate) identity: Option<crate::contract::ReceiverIdentity>,
    pub(crate) at: Option<String>,
    pub(crate) evidence: Option<String>,
    pub(crate) gateway: Option<String>,
    pub(crate) shape: Option<Shape>,
    pub(crate) gateways: Vec<Gateway>,
    pub(crate) hosts: Vec<String>,
    pub(crate) version: Option<String>,
    pub(crate) cap: Option<crate::contract::Level>,
    pub(crate) client: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct Gateway {
    pub(crate) path: String,
    pub(crate) reason: String,
}

#[derive(Clone)]
pub(crate) struct Consumer {
    pub(crate) contract: Option<String>,
    pub(crate) site: String,
    pub(crate) via: Option<String>,
    pub(crate) shape: Option<Shape>,
    pub(crate) method: Option<String>,
    pub(crate) identity: Option<crate::contract::ReceiverIdentity>,
    pub(crate) fragments: Vec<String>,
    pub(crate) lead: Option<crate::contract::Lead>,
    pub(crate) hops: Vec<Hop>,
    pub(crate) root: Option<String>,
    pub(crate) cap: Option<crate::contract::Level>,
    pub(crate) client: Option<String>,
}

impl Consumer {
    pub(crate) fn item_site(&self) -> std::borrow::Cow<'_, str> {
        written::side_id(
            &self.site,
            self.contract.as_deref().unwrap_or_default(),
            self.root.as_deref(),
        )
    }
}

fn stored_shape(json: Option<String>) -> Option<Shape> {
    // ponytail: written by `refresh` only, so a bad value is an index to rebuild.
    serde_json::from_str(&json?).ok()
}

fn stored_level(name: Option<String>) -> Option<crate::contract::Level> {
    serde_json::from_value(serde_json::Value::String(name?)).ok()
}

fn stored_identity(json: Option<String>) -> Option<crate::contract::ReceiverIdentity> {
    serde_json::from_str(&json?).ok()
}

pub fn export(db: &Connection, identity: &str) -> Result<Export> {
    let mut export = contracts(db, identity)?;
    export.symbols = symbols_where(db, "", "[]")?;
    export.calls = calls_where(db, "", "[]", &|_| false)?;
    let mut communities: Vec<Community> = Vec::new();
    for row in db
        .prepare("SELECT name, member FROM communities ORDER BY name, member")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?
    {
        let (name, member) = row?;
        match communities.last_mut() {
            Some(c) if c.name == name => c.members.push(member),
            _ => {
                let mut community = Community::default();
                community.name = name;
                community.members = vec![member];
                communities.push(community);
            }
        }
    }
    export.communities = communities;
    Ok(export)
}

pub(crate) fn contracts(db: &Connection, identity: &str) -> Result<Export> {
    let files = db
        .prepare("SELECT path, hash FROM files WHERE resolved != '' ORDER BY path")?
        .query_map([], |r| {
            let mut file = File::default();
            file.path = r.get(0)?;
            file.hash = r.get(1)?;
            Ok(file)
        })?
        .collect::<rusqlite::Result<_>>()?;
    let dropped = db
        .prepare(
            "SELECT path, facts FROM files WHERE NOT EXISTS \
             (SELECT 1 FROM symbols s WHERE s.path = files.path) \
             AND (instr(facts, '\"budget\":') > 0 OR instr(facts, '\"usings\":') > 0 \
                  OR instr(facts, '\"misparsed\":') > 0) \
             ORDER BY path",
        )?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|(path, facts)| {
            let facts: Facts = serde_json::from_str(&facts).ok()?;
            let limit = match (facts.budget, facts.usings) {
                (Some(bytes), _) => Dropped::Bytes(bytes),
                (None, Some(count)) => Dropped::Usings(count),
                (None, None) if facts.misparsed => Dropped::Misparsed,
                (None, None) => return None,
            };
            Some((path, limit))
        })
        .collect();
    let providers = db
        .prepare(
            "SELECT contract, site, shape, via, method, identity, cap, client FROM contracts \
             WHERE role = 'provider' ORDER BY contract, site",
        )?
        .query_map([], |r| {
            Ok(Provider {
                contract: r.get(0)?,
                site: r.get(1)?,
                shape: stored_shape(r.get(2)?),
                via: r.get(3)?,
                method: r.get(4)?,
                identity: stored_identity(r.get(5)?),
                cap: stored_level(r.get(6)?),
                client: r.get(7)?,
                ..Provider::default()
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let consumers = db
        .prepare(
            "SELECT contract, site, via, method, fragments, shape, identity, lead, cap, client \
             FROM contracts WHERE role = 'consumer' ORDER BY site, contract, via",
        )?
        .query_map([], |r| {
            let fragments: Option<String> = r.get(4)?;
            Ok(Consumer {
                identity: stored_identity(r.get(6)?),
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
                root: None,
                lead: r
                    .get::<_, Option<String>>(7)?
                    .and_then(|l| serde_json::from_str(&l).ok()),
                cap: stored_level(r.get(8)?),
                client: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let bindings = db
        .prepare(
            "SELECT contract, via, method, site FROM contracts \
             WHERE role = 'binding' ORDER BY contract, via, method, site",
        )?
        .query_map([], |r| {
            let mut binding = Binding::default();
            binding.contract = r.get(0)?;
            binding.exchange = r.get(1)?;
            binding.pattern = r.get(2)?;
            binding.site = r.get(3)?;
            Ok(binding)
        })?
        .collect::<rusqlite::Result<_>>()?;
    let rows: Vec<(String, String)> = db
        .prepare("SELECT path, summary FROM documents ORDER BY path")?
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
    let unmounted = db
        .prepare("SELECT site FROM contracts WHERE role = 'unmounted' ORDER BY site")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let unfollowed = db
        .prepare(
            "SELECT via, fragments, method FROM contracts WHERE role = 'unfollowed' \
             ORDER BY via, fragments, method",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let publishes = db
        .prepare(
            "SELECT via, method, site FROM contracts \
             WHERE role = 'publish' ORDER BY via, method, site",
        )?
        .query_map([], |r| {
            let mut publish = Publish::default();
            publish.exchange = r.get(0)?;
            publish.key = r.get(1)?;
            publish.site = r.get(2)?;
            Ok(publish)
        })?
        .collect::<rusqlite::Result<_>>()?;
    let rows: Vec<(String, String)> = db
        .prepare("SELECT path, summary FROM manifests ORDER BY path")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let (manifests, tsconfigs) = read_manifests(rows)?;
    let resolved = resolved_rows(db, &manifests)?;
    let import_calls = match manifests.is_empty() {
        true => Vec::new(),
        false => db
            .prepare(
                "SELECT site, path, via, method FROM contracts \
                 WHERE role = 'import' ORDER BY site, via, method",
            )?
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
    let provides = manifests.iter().any(|m| m.summary.name.is_some());
    let exported = match provides {
        false => Vec::new(),
        true => db
            .prepare(
                "SELECT path, name, file, target, source FROM exports \
                 ORDER BY path, name, file, target",
            )?
            .query_map([], |r| {
                Ok(Exported {
                    path: r.get(0)?,
                    name: r.get(1)?,
                    file: r.get(2)?,
                    target: r.get(3)?,
                    source: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?,
    };
    let ancestries = match provides {
        false => Vec::new(),
        true => db
            .prepare("SELECT path, class, ancestors FROM families ORDER BY path, class")?
            .query_map([], |r| {
                let ancestors: Option<String> = r.get(2)?;
                Ok(Ancestry {
                    path: r.get(0)?,
                    class: r.get(1)?,
                    // ponytail: written by `refresh` only; an unreadable family is one not known.
                    ancestors: ancestors.and_then(|a| serde_json::from_str(&a).ok()),
                })
            })?
            .collect::<rusqlite::Result<_>>()?,
    };
    let flexible = match provides {
        false => Vec::new(),
        true => db
            .prepare("SELECT path, name, least, most FROM flexible ORDER BY path, name")?
            .query_map([], |r| {
                Ok(Flexible {
                    path: r.get(0)?,
                    name: r.get(1)?,
                    least: r.get::<_, i64>(2)? as usize,
                    most: r.get::<_, Option<i64>>(3)?.map(|m| m as usize),
                })
            })?
            .collect::<rusqlite::Result<_>>()?,
    };
    let shapes = match provides {
        false => Vec::new(),
        true => db
            .prepare(
                "SELECT path, method, via, shape FROM contracts \
                 WHERE role = 'shape' ORDER BY path, method, via",
            )?
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
    let entry_points = db
        .prepare("SELECT identity, kind, label, site FROM entry_points ORDER BY identity")?
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
    let (config_uses, policy_grants, scripts) = iac::config_uses::read(db, identity)?;
    Ok(Export {
        repository: identity.to_string(),
        root: PathBuf::new(),
        commit: None,
        files,
        dropped,
        symbols: Vec::new(),
        calls: Vec::new(),
        providers,
        consumers,
        bindings,
        provision_uses: Vec::new(),
        publishes,
        manifests,
        tsconfigs,
        packages: Vec::new(),
        dependencies: Vec::new(),
        resolved,
        import_calls,
        exported,
        ancestries,
        flexible,
        shapes,
        imports: Vec::new(),
        communities: Vec::new(),
        links: Vec::new(),
        provisioners: iac::provision::read(db, identity)?,
        config_uses,
        scripts,
        policy_grants,
        config_sites: Vec::new(),
        configurations: db
            .prepare("SELECT path, summary FROM contract_configs ORDER BY path")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .map(|r| {
                let (p, s) = r?;
                Ok((p, serde_json::from_str(&s)?))
            })
            .collect::<Result<_>>()?,
        iac: iac::nodes(db)?,
        iac_references: iac::all_references(db)?,
        dockerfiles: stored_dockerfiles(db)?,
        http: None,
        workload_images: db
            .prepare(
                "SELECT root, address, image FROM iac_images ORDER BY root, address, container",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?,
        executions: Vec::new(),
        subscriptions: iac::subscription::read(db, identity)?,
        provisions: Vec::new(),
        decisions: crate::decisions::Decisions::default(),
        name_candidates: std::collections::BTreeMap::new(),
        entry_points,
        processes: Vec::new(),
        documents,
        stale_decisions: Vec::new(),
        unmounted,
        unfollowed,
    })
}

fn read_manifests(rows: Vec<(String, String)>) -> Result<(Vec<Manifest>, Vec<Config>)> {
    let (mut manifests, mut tsconfigs) = (Vec::new(), Vec::new());
    for (path, summary) in rows {
        match package::is_tsconfig(&path) {
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
    package::inherit(&mut manifests);
    Ok((manifests, tsconfigs))
}

fn span(row: &rusqlite::Row, at: usize) -> rusqlite::Result<Span> {
    let mut span = Span::default();
    span.path = row.get(at)?;
    span.start = row.get(at + 1)?;
    span.end = row.get(at + 2)?;
    Ok(span)
}

fn listed<S: AsRef<str>>(values: &[S]) -> String {
    let values: Vec<&str> = values.iter().map(AsRef::as_ref).collect();
    serde_json::to_string(&values).unwrap_or_else(|_| "[]".into())
}

fn symbols_where(db: &Connection, filter: &str, list: &str) -> Result<Vec<Symbol>> {
    let sql = format!(
        "SELECT id, path, start, end, hash FROM symbols {filter} ORDER BY id, path, start, end"
    );
    let mut statement = db.prepare(&sql)?;
    let bound = statement.parameter_count();
    let rows = match bound {
        0 => statement.query([])?,
        _ => statement.query(params![list])?,
    };
    Ok(rows.mapped(symbol).collect::<rusqlite::Result<_>>()?)
}

fn symbol(r: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    let mut symbol = Symbol::default();
    symbol.id = r.get(0)?;
    symbol.span = span(r, 1)?;
    symbol.hash = r.get(4)?;
    Ok(symbol)
}

fn calls_where(
    db: &Connection,
    filter: &str,
    list: &str,
    held: &dyn Fn(&str) -> bool,
) -> Result<Vec<CallSite>> {
    let sql = format!(
        "SELECT id, caller, target, level, path, start, end, narrowed, decorates, kind, holder \
         FROM calls \
         {filter} ORDER BY id, path, start, end"
    );
    let mut statement = db.prepare(&sql)?;
    let rows = match statement.parameter_count() {
        0 => statement.query([])?,
        _ => statement.query(params![list])?,
    };
    let calls: Vec<CallSite> = rows
        .mapped(|r| {
            let mut call = CallSite::default();
            call.id = r.get(0)?;
            call.caller = r.get(1)?;
            call.target = r.get(2)?;
            call.level = r
                .get::<_, Option<String>>(3)?
                .as_deref()
                .and_then(Level::parse);
            call.span = span(r, 4)?;
            call.narrowed = r.get(7)?;
            call.decorates = r.get(8)?;
            call.kind = r.get(9)?;
            call.holder = r.get(10)?;
            Ok(call)
        })
        .collect::<rusqlite::Result<_>>()?;
    let mut candidates;
    let rows = match filter.is_empty() {
        true => {
            candidates = db.prepare("SELECT call, target FROM candidates ORDER BY call, target")?;
            candidates.query([])?
        }
        false => {
            candidates = db.prepare(
                "SELECT call, target FROM candidates \
                 WHERE call IN (SELECT value FROM json_each(?1)) ORDER BY call, target",
            )?;
            let ids: Vec<&str> = (calls.iter().map(|c| c.id.as_str()))
                .filter(|id| !held(id))
                .collect();
            candidates.query(params![listed(&ids)])?
        }
    };
    let mut by_call: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows.mapped(|r| Ok((r.get::<_, String>(0)?, r.get(1)?))) {
        let (call, target) = row?;
        by_call.entry(call).or_default().push(target);
    }
    Ok(calls
        .into_iter()
        .map(|mut call| {
            call.candidates = by_call.remove(&call.id).unwrap_or_default();
            call
        })
        .collect())
}

pub(crate) fn symbols_of<S: AsRef<str>>(db: &Connection, ids: &[S]) -> Result<Vec<Symbol>> {
    let filter = "WHERE id IN (SELECT value FROM json_each(?1))";
    symbols_where(db, filter, &listed(ids))
}

pub(crate) fn symbols_in<S: AsRef<str>>(db: &Connection, paths: &[S]) -> Result<Vec<Symbol>> {
    let filter = "WHERE path IN (SELECT value FROM json_each(?1))";
    symbols_where(db, filter, &listed(paths))
}

pub(crate) fn symbols_hashed<S: AsRef<str>>(db: &Connection, hashes: &[S]) -> Result<Vec<Symbol>> {
    let filter = "WHERE hash IN (SELECT value FROM json_each(?1))";
    symbols_where(db, filter, &listed(hashes))
}

fn named(name: &str) -> (String, String) {
    let suffix = format!("::{name}");
    let filter = format!(
        "WHERE id = json_extract(?1, '$[0]') OR substr(id, -{}) = json_extract(?1, '$[1]')",
        suffix.chars().count()
    );
    (filter, listed(&[name, suffix.as_str()]))
}

pub(crate) fn symbols_named(db: &Connection, name: &str) -> Result<Vec<Symbol>> {
    let (filter, list) = named(name);
    symbols_where(db, &filter, &list)
}

pub(crate) fn calls_named(db: &Connection, name: &str) -> Result<Vec<CallSite>> {
    let (filter, list) = named(name);
    calls_where(db, &filter, &list, &|_| false)
}

pub(crate) fn each_symbol(db: &Connection, mut f: impl FnMut(Symbol)) -> Result<()> {
    let mut statement =
        db.prepare("SELECT id, path, start, end, hash FROM symbols ORDER BY id, path, start, end")?;
    for row in statement.query([])?.mapped(symbol) {
        f(row?);
    }
    Ok(())
}

pub(crate) fn calls_of<S: AsRef<str>>(
    db: &Connection,
    ids: &[S],
    held: &dyn Fn(&str) -> bool,
) -> Result<Vec<CallSite>> {
    let filter = "WHERE id IN (SELECT value FROM json_each(?1))";
    calls_where(db, filter, &listed(ids), held)
}

pub(crate) fn calls_by<S: AsRef<str>>(
    db: &Connection,
    callers: &[S],
    held: &dyn Fn(&str) -> bool,
) -> Result<Vec<CallSite>> {
    let filter = "WHERE caller IN (SELECT value FROM json_each(?1))";
    calls_where(db, filter, &listed(callers), held)
}

pub(crate) fn uses_held<S: AsRef<str>>(
    db: &Connection,
    holders: &[S],
    held: &dyn Fn(&str) -> bool,
) -> Result<Vec<CallSite>> {
    let filter = "WHERE kind IS NOT NULL AND holder IN (SELECT value FROM json_each(?1))";
    calls_where(db, filter, &listed(holders), held)
}

pub(crate) fn calls_to<S: AsRef<str>>(
    db: &Connection,
    targets: &[S],
    candidates: bool,
    held: &dyn Fn(&str) -> bool,
) -> Result<Vec<CallSite>> {
    let filter = match candidates {
        false => "WHERE target IN (SELECT value FROM json_each(?1))",
        true => {
            "WHERE rowid IN (SELECT rowid FROM calls WHERE target IN \
             (SELECT value FROM json_each(?1)) UNION SELECT c.rowid FROM candidates k \
             JOIN calls c ON c.id = k.call WHERE k.target IN (SELECT value FROM json_each(?1)))"
        }
    };
    calls_where(db, filter, &listed(targets), held)
}

pub(crate) fn community_of(db: &Connection, member: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT name FROM communities WHERE member = ?1 ORDER BY name LIMIT 1",
            [member],
            |r| r.get(0),
        )
        .optional()?)
}

pub(crate) fn community_sizes(db: &Connection) -> Result<Vec<(String, usize)>> {
    let mut statement =
        db.prepare("SELECT name, count(*) FROM communities GROUP BY name ORDER BY name")?;
    let rows = statement
        .query([])?
        .mapped(|r| Ok((r.get(0)?, r.get::<_, i64>(1)? as usize)));
    Ok(rows.collect::<rusqlite::Result<_>>()?)
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
        SCHEMA, State, open, open_read, read_config, refresh_stamped, state, walk,
        with_busy_timeout,
    };
    use crate::config::Serve;
    use crate::kube;
    use crate::msg::Fix;
    use crate::{config_values, document, lang, package};

    const REPO: &str = "example.com/acme/web";
    const MANIFEST: &str = "ops/meada.workspace.toml";

    fn index_dir(case: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("meada-index-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let index = dir.join(REPO).join("index.db");
        (dir, index)
    }

    fn read(index: &Path) -> crate::Result<Connection> {
        open_read(index, REPO, Fix::Index(MANIFEST))
    }

    fn communities(db: &Connection) -> i64 {
        (db.query_row("SELECT count(*) FROM communities", [], |r| r.get(0))).unwrap()
    }

    #[test]
    fn facts_loading_scans_once_for_many_names() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(super::SCHEMA).unwrap();
        for i in 0..300 {
            let facts = crate::lang::Facts {
                loads: (0..60)
                    .map(|k| format!("padding-{i}-{k}/sub"))
                    .chain([format!("pkg{i}")])
                    .collect(),
                ..Default::default()
            };
            let facts = serde_json::to_string(&facts).unwrap();
            db.execute(
                "INSERT INTO files (path, hash, size, facts, resolved, setting) VALUES (?1, '', 0, ?2, '', '')",
                rusqlite::params![format!("src/f{i}.ts"), facts],
            )
            .unwrap();
        }
        let names: Vec<String> = (0..100_000)
            .map(|i| format!("n{i}"))
            .chain(["pkg7".into()])
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let start = Instant::now();
        let found = super::facts_loading(&db, &names).unwrap();
        let took = start.elapsed();
        assert_eq!(found.keys().collect::<Vec<_>>(), ["src/f7.ts"]);
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    #[test]
    fn a_read_only_open_waits_for_a_commit_in_progress_up_to_the_busy_timeout() {
        let (dir, index) = index_dir("busy");
        let writer = open(&index, &dir, REPO, &Serve::default()).unwrap();
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

    fn older_index(index: &Path) {
        fs::create_dir_all(index.parent().unwrap()).unwrap();
        let db = Connection::open(index).unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.execute_batch("INSERT INTO communities VALUES ('old', 'm'); PRAGMA user_version = 2;")
            .unwrap();
    }

    fn checkout(dir: &Path) -> PathBuf {
        let root = dir.join("checkout");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.ts"), "export function a() {\n  return 1;\n}\n").unwrap();
        root
    }

    fn files(db: &Connection) -> i64 {
        (db.query_row("SELECT count(*) FROM files", [], |r| r.get(0))).unwrap()
    }

    fn named(db: &Connection, name: &str) -> i64 {
        let count = "SELECT count(*) FROM symbols WHERE id LIKE '%::' || ?1";
        (db.query_row(count, [name], |r| r.get(0))).unwrap()
    }

    #[test]
    fn a_refresh_with_nothing_to_write_runs_while_another_holds_the_write_lock() {
        use super::refresh;
        let (dir, index) = index_dir("refresh_beside_a_writer");
        let root = checkout(&dir);
        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        let writer = Connection::open(&index).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();

        let refreshed = with_busy_timeout(Duration::from_millis(50), || {
            refresh(&db, &root, REPO, &|| Ok(Serve::default()))
        });
        assert!(refreshed.is_ok(), "{refreshed:?}");
        assert_eq!(named(&db, "a"), 1);
    }

    #[test]
    fn a_refresh_that_meets_another_writer_waits_for_it_and_writes_its_change() {
        use super::refresh;
        let (dir, index) = index_dir("refresh_after_a_writer");
        let root = checkout(&dir);
        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        fs::write(root.join("a.ts"), "export function b() {\n  return 2;\n}\n").unwrap();
        let writer = Connection::open(&index).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        let ends = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            writer.execute_batch("ROLLBACK").unwrap();
        });

        let refreshed = with_busy_timeout(Duration::from_secs(10), || {
            refresh(&db, &root, REPO, &|| Ok(Serve::default()))
        });
        ends.join().unwrap();
        assert!(refreshed.is_ok(), "{refreshed:?}");
        assert_eq!((named(&db, "a"), named(&db, "b")), (0, 1));
    }

    fn connections(index: &Path, root: &Path, n: usize) -> Vec<Connection> {
        (0..n)
            .map(|_| open(index, root, REPO, &Serve::default()).unwrap())
            .collect()
    }

    fn refreshing(
        db: Connection,
        root: &Path,
        busy: Duration,
        times: usize,
    ) -> std::thread::JoinHandle<Result<(), String>> {
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            with_busy_timeout(busy, || {
                (0..times).try_for_each(|_| {
                    (super::refresh(&db, &root, REPO, &|| Ok(Serve::default())))
                        .map_err(|e| e.to_string())
                })
            })
        })
    }

    fn reader(index: &Path) -> Connection {
        let db = Connection::open_with_flags(index, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        db.execute_batch("BEGIN").unwrap();
        files(&db);
        db
    }

    fn committing(index: &Path) {
        let probe = Connection::open_with_flags(index, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        probe.busy_timeout(Duration::ZERO).unwrap();
        let start = Instant::now();
        while (probe.query_row("SELECT count(*) FROM files", [], |r| r.get::<_, i64>(0))).is_ok() {
            assert!(start.elapsed() < Duration::from_secs(10), "no commit began");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn add(root: &Path, name: &str) {
        let source = format!("export function {name}() {{\n  return 1;\n}}\n");
        fs::write(root.join(format!("{name}.ts")), source).unwrap();
    }

    #[test]
    fn refreshes_of_one_process_wait_for_the_run_in_flight_with_no_timer() {
        let (dir, index) = index_dir("refresh_in_flight");
        let root = checkout(&dir);
        let dbs = connections(&index, &root, 8);
        for i in 0..200 {
            add(&root, &format!("f{i}"));
        }

        let runs: Vec<_> = (dbs.into_iter())
            .map(|db| refreshing(db, &root, Duration::ZERO, 1))
            .collect();
        for run in runs {
            assert_eq!(run.join().unwrap(), Ok(()));
        }
        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        assert_eq!((named(&db, "a"), named(&db, "f199")), (1, 1));
    }

    #[test]
    fn a_call_waits_past_the_busy_timeout_for_the_refresh_in_flight_in_its_process() {
        let (dir, index) = index_dir("refresh_outlasts_timeout");
        let root = checkout(&dir);
        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        add(&root, "b");
        let blocker = reader(&index);
        let run = refreshing(db, &root, Duration::from_secs(10), 1);
        committing(&index);

        let call = std::thread::spawn({
            let (index, root) = (index.clone(), root.clone());
            move || {
                with_busy_timeout(Duration::from_millis(50), || {
                    let db = open(&index, &root, REPO, &Serve::default());
                    db.map(|db| named(&db, "b")).map_err(|e| e.to_string())
                })
            }
        });
        std::thread::sleep(Duration::from_millis(400));
        let waiting = !call.is_finished();
        drop(blocker);

        assert_eq!(run.join().unwrap(), Ok(()));
        assert_eq!((call.join().unwrap(), waiting), (Ok(1), true));
    }

    #[test]
    fn a_lock_no_refresh_holds_fails_once_the_refreshes_in_flight_end() {
        let (dir, index) = index_dir("refresh_elsewhere");
        let root = checkout(&dir);
        let writer = open(&index, &root, REPO, &Serve::default()).unwrap();
        writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let other = dir.join("other").join("index.db");
        let db = open(&other, &root, REPO, &Serve::default()).unwrap();
        add(&root, "b");
        let blocker = reader(&other);
        let run = refreshing(db, &root, Duration::from_millis(300), 1);
        committing(&other);

        let start = Instant::now();
        let second = with_busy_timeout(Duration::from_millis(50), || {
            open(&index, &root, REPO, &Serve::default())
        });
        let waited = start.elapsed();

        assert!(second.is_err());
        assert!(waited >= Duration::from_millis(300), "{waited:?}");
        assert!(run.join().unwrap().is_err());
        drop(blocker);
    }

    #[test]
    fn a_wait_that_covered_a_run_of_its_process_keeps_a_whole_timeout_after_it() {
        let (dir, index) = index_dir("wait_after_run");
        let root = checkout(&dir);
        let mut dbs = connections(&index, &root, 2);
        add(&root, "b");
        let blocker = reader(&index);
        let run = refreshing(dbs.remove(0), &root, Duration::from_secs(10), 1);
        committing(&index);
        let other = std::thread::spawn({
            let index = index.clone();
            move || {
                let writer = Connection::open(&index).unwrap();
                writer.busy_handler(Some(|_| true)).unwrap();
                writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
                std::thread::sleep(Duration::from_millis(100));
                writer.execute_batch("ROLLBACK").unwrap();
            }
        });
        let call = dbs.remove(0);
        let statement = std::thread::spawn(move || {
            with_busy_timeout(Duration::from_millis(300), || {
                let count = "SELECT count(*) FROM symbols WHERE id LIKE '%::b'";
                (call.query_row(count, [], |r| r.get::<_, i64>(0))).map_err(|e| e.to_string())
            })
        });
        std::thread::sleep(Duration::from_millis(600));
        drop(blocker);

        assert_eq!(run.join().unwrap(), Ok(()));
        other.join().unwrap();
        assert_eq!(statement.join().unwrap(), Ok(1));
    }

    #[test]
    fn a_wait_that_ended_takes_nothing_from_the_next_one() {
        let (dir, index) = index_dir("wait_again");
        let root = checkout(&dir);
        drop(open(&index, &root, REPO, &Serve::default()).unwrap());
        let hold = || {
            let writer = Connection::open(&index).unwrap();
            writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                writer.execute_batch("ROLLBACK").unwrap();
            })
        };
        let timeout = Duration::from_secs(1);

        let found = with_busy_timeout(timeout, || {
            let first = hold();
            let db = open(&index, &root, REPO, &Serve::default()).map_err(|e| e.to_string())?;
            first.join().unwrap();
            std::thread::sleep(timeout);
            let second = hold();
            let count = "SELECT count(*) FROM symbols WHERE id LIKE '%::a'";
            let found = db.query_row(count, [], |r| r.get::<_, i64>(0));
            second.join().unwrap();
            found.map_err(|e| e.to_string())
        });

        assert_eq!(found, Ok(1));
    }

    #[test]
    fn a_call_that_comes_during_a_run_answers_from_the_next_run() {
        let (dir, index) = index_dir("refresh_next_run");
        let root = checkout(&dir);
        let mut dbs = connections(&index, &root, 2);
        add(&root, "b");
        let blocker = reader(&index);
        let first = refreshing(dbs.remove(0), &root, Duration::from_secs(10), 1);
        committing(&index);

        add(&root, "c");
        let second = refreshing(dbs.remove(0), &root, Duration::from_secs(10), 1);
        std::thread::sleep(Duration::from_millis(200));
        drop(blocker);

        assert_eq!(
            (first.join().unwrap(), second.join().unwrap()),
            (Ok(()), Ok(()))
        );
        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        assert_eq!((named(&db, "b"), named(&db, "c")), (1, 1));
    }

    #[test]
    fn a_run_reads_the_serve_table_when_it_starts_not_when_its_call_came() {
        let (dir, index) = index_dir("serve_at_run_start");
        let root = checkout(&dir);
        fs::create_dir_all(root.join("docs")).unwrap();
        let spec = "openapi: 3.0.0\ninfo:\n  title: api\npaths: {}\n";
        fs::write(root.join("docs").join("api.yaml"), spec).unwrap();
        fs::write(root.join("meada.toml"), "[serve]\n").unwrap();
        let mut dbs = connections(&index, &root, 2);
        add(&root, "b");
        let blocker = reader(&index);
        let first = refreshing(dbs.remove(0), &root, Duration::from_secs(10), 1);
        committing(&index);

        let second = std::thread::spawn({
            let (db, root) = (dbs.remove(0), root.clone());
            move || {
                let serve = || -> crate::Result<Serve> { Ok(crate::config::load(&root)?.serve) };
                with_busy_timeout(Duration::from_secs(10), || {
                    super::refresh(&db, &root, REPO, &serve)
                })
                .map_err(|e| e.to_string())
            }
        });
        std::thread::sleep(Duration::from_millis(200));
        let edited = "[serve]\ndocuments = [\"docs/api.yaml\"]\n";
        fs::write(root.join("meada.toml"), edited).unwrap();
        drop(blocker);

        assert_eq!(
            (first.join().unwrap(), second.join().unwrap()),
            (Ok(()), Ok(()))
        );
        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        let listed = "SELECT count(*) FROM documents WHERE path = 'docs/api.yaml'";
        assert_eq!(db.query_row(listed, [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn a_refresh_refused_at_every_deferred_attempt_takes_the_write_lock_first() {
        let db = Connection::open_in_memory().unwrap();
        let mut seen = Vec::new();

        let outcome = super::attempts(&db, |behavior| {
            seen.push(matches!(behavior, rusqlite::TransactionBehavior::Immediate));
            if seen.len() > super::DEFERRED_ATTEMPTS {
                return Ok(());
            }
            let refused = rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY);
            Err(rusqlite::Error::SqliteFailure(refused, None).into())
        });

        assert!(outcome.is_ok(), "{outcome:?}");
        let deferred = vec![false; super::DEFERRED_ATTEMPTS];
        assert_eq!(seen, [deferred, vec![true]].concat());
    }

    #[test]
    fn a_tree_that_keeps_changing_never_fails_the_refreshes_of_one_process() {
        let (dir, index) = index_dir("refresh_changing");
        let root = checkout(&dir);
        let dbs = connections(&index, &root, 8);
        let stop = Arc::new(AtomicBool::new(false));
        let edits = std::thread::spawn({
            let (root, stop) = (root.clone(), stop.clone());
            move || {
                for i in 0.. {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let source = format!("export function c{i}() {{\n  return {i};\n}}\n");
                    fs::write(root.join("changing.ts"), source).unwrap();
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });

        let runs: Vec<_> = (dbs.into_iter())
            .map(|db| refreshing(db, &root, Duration::ZERO, 10))
            .collect();
        let outcomes: Vec<_> = runs.into_iter().map(|run| run.join().unwrap()).collect();
        stop.store(true, Ordering::Relaxed);
        edits.join().unwrap();
        assert!(outcomes.iter().all(Result::is_ok), "{outcomes:?}");
    }

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

    #[test]
    fn the_walk_skips_build_output_beside_a_project_file() {
        let (dir, _) = index_dir("build_output");
        for (path, body) in [
            ("App/App.csproj", "<Project/>"),
            ("App/Program.cs", ""),
            ("App/obj/Debug/net8.0/App.AssemblyInfo.cs", ""),
            ("App/bin/Debug/x.cs", ""),
            ("tools/obj/Thing.cs", ""),
        ] {
            let file = dir.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, body).unwrap();
        }
        let seen: Vec<String> = walk(&dir).into_iter().map(|(p, _)| p).collect();
        assert_eq!(
            seen,
            ["App/App.csproj", "App/Program.cs", "tools/obj/Thing.cs"]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_walk_skips_dependency_caches_and_minified_assets() {
        let (dir, _) = index_dir("default_skips");
        for path in [
            "src/a.js",
            "node_modules/x/a.js",
            "ios/Pods/x/a.swift",
            "Carthage/Checkouts/x/a.swift",
            "Checkouts/own.swift",
            "static/app.min.js",
            "static/app.min.css",
            "static/app.css",
            "vendor/a.js",
        ] {
            let file = dir.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, "").unwrap();
        }
        let seen: Vec<String> = walk(&dir).into_iter().map(|(p, _)| p).collect();
        assert_eq!(
            seen,
            [
                "Checkouts/own.swift",
                "src/a.js",
                "static/app.css",
                "vendor/a.js"
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

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

    fn interrupted(dir: &Path, name: &str) -> PathBuf {
        let index = dir.join(name).join("source/index.db");
        let writer = open(&index, dir, REPO, &Serve::default()).unwrap();
        writer
            .execute_batch("INSERT INTO communities VALUES ('c', 'm');")
            .unwrap();
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

    #[cfg(unix)]
    #[test]
    fn a_read_only_open_over_an_interrupted_commit_on_a_read_only_mount_names_the_fix() {
        use std::os::unix::fs::PermissionsExt;
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
        assert_eq!(left, 3);
        let _ = fs::remove_dir_all(&dir);
    }

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

    #[test]
    fn a_rebuild_removes_a_journal_left_beside_the_index() {
        let (dir, index) = index_dir("stray_journal");
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

            assert_eq!(communities(&db), 0, "older: {older}");
            assert_eq!(files(&db), 1, "older: {older}");
            assert_eq!(files(&read(&index).unwrap()), 1, "older: {older}");
            assert!(!index.with_file_name("index.db-journal").exists());
        }
        let _ = fs::remove_dir_all(&dir);
    }

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
            ("src".to_string(), root.join("src")),
            ("Cargo.toml".to_string(), root.join("Cargo.toml")),
        ];
        let tx = db.transaction().unwrap();

        refresh_stamped(
            &tx,
            "manifests",
            &found,
            std::time::SystemTime::now(),
            |_| Ok(false),
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

    #[test]
    fn the_stored_format_matches_its_golden() {
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut actual = format!(
            "extraction revision {}\n\n{}",
            super::EXTRACTION_REVISION,
            SCHEMA.trim_start()
        );
        let identity = [
            crate::contract::ReceiverIdentity::Sdk {
                kind: "subscription".into(),
                parts: vec![
                    crate::names::Argument::Literal("orders".into()),
                    crate::names::Argument::Environment("SUBSCRIPTION".into()),
                ],
            },
            crate::contract::ReceiverIdentity::Sdk {
                kind: "entity".into(),
                parts: vec![crate::names::Argument::Unresolved],
            },
        ];
        actual += &format!(
            "\nreceiver_identity\n{}\n",
            serde_json::to_string_pretty(&identity).unwrap()
        );
        let lead = |limit: lang::Limit, path: &str| crate::contract::Lead {
            limit,
            path: path.into(),
        };
        let leads = [
            lead(lang::Limit::Returned("c.baseV2".into()), "/orders"),
            lead(lang::Limit::Parameter("base".into()), "/items/{}"),
            lead(lang::Limit::ClientBase, "/orders"),
            lead(lang::Limit::Other, "/"),
        ];
        actual += &format!(
            "\nlead\n{}\n",
            serde_json::to_string_pretty(&leads).unwrap()
        );
        for pack in lang::packs() {
            let name = format!("sample.{}", pack.extensions()[0]);
            let src = fs::read(tests.join("fixtures/schema").join(&name)).unwrap_or_else(|_| {
                panic!("add tests/fixtures/schema/{name}, a sample of the facts its pack stores")
            });
            let mut facts = pack.parse(&name, &src).unwrap();
            lang::hash_spans(&mut facts, &src);
            let facts = serde_json::to_string_pretty(&facts).unwrap();
            actual += &format!("\n{name}\n{facts}\n");
        }
        let src = fs::read(tests.join("fixtures/schema/sample.tsx")).unwrap();
        let mut facts =
            lang::LanguagePack::parse(&crate::lang::ts::TsJs, "sample.tsx", &src).unwrap();
        lang::hash_spans(&mut facts, &src);
        let facts = serde_json::to_string_pretty(&facts).unwrap();
        actual += &format!("\nsample.tsx\n{facts}\n");
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
            ("sample.pom.xml", "pom.xml"),
            ("sample.build.gradle.kts", "build.gradle.kts"),
            ("sample.settings.gradle.kts", "settings.gradle.kts"),
            ("sample.gradle.properties", "gradle.properties"),
            ("sample.libs.versions.toml", "gradle/libs.versions.toml"),
            ("sample.csproj", "src/Kit/Kit.csproj"),
            ("sample.Directory.Build.props", "src/Directory.Build.props"),
            ("sample.Package.swift", "pkg/Package.swift"),
        ] {
            let src = fs::read(tests.join("fixtures/schema").join(name)).unwrap();
            let summary = package::read_manifest(path, &src);
            let summary = serde_json::to_string_pretty(&summary).unwrap();
            actual += &format!("\n{name}\n{summary}\n");
        }
        let src = fs::read(tests.join("fixtures/schema/sample.tsconfig.json")).unwrap();
        let summary = serde_json::to_string_pretty(&package::read_tsconfig(&src)).unwrap();
        actual += &format!("\nsample.tsconfig.json\n{summary}\n");
        let src = fs::read(tests.join("fixtures/schema/sample.values.yaml")).unwrap();
        let (outcome, rows) = config_values::read("sample.values.yaml", &src);
        let outcome = serde_json::to_string_pretty(&outcome).unwrap();
        actual += &format!("\nsample.values.yaml\n{outcome}\n");
        actual.extend(rows.iter().map(|row| format!("{row:?}\n")));
        let (outcome, rows) =
            config_values::read("sample.data.yaml", past_the_ceiling("").as_bytes());
        let outcome = serde_json::to_string_pretty(&outcome).unwrap();
        actual += &format!("\nsample.data.yaml, one value past the ceiling\n{outcome}\n");
        actual.extend(rows.iter().map(|row| format!("{row:?}\n")));

        let files: Vec<(String, Vec<u8>)> = [
            ("sample.kustomization.yaml", "overlay/kustomization.yaml"),
            ("sample.kubernetes.yaml", "overlay/sample.kubernetes.yaml"),
        ]
        .iter()
        .map(|(name, path)| {
            (
                path.to_string(),
                fs::read(tests.join("fixtures/schema").join(name)).unwrap(),
            )
        })
        .collect();
        actual += "\nsample.kustomization.yaml\n";
        actual += &format!("{:#?}\n", kube::evaluate(&files));

        let src = fs::read(tests.join("fixtures/schema/sample.crossplane.yaml")).unwrap();
        let files = [("xp/sample.crossplane.yaml".to_string(), src)];
        actual += "\nsample.crossplane.yaml\n";
        actual += &format!("{:#?}\n", kube::evaluate(&files));
        let src = fs::read(tests.join("fixtures/schema/sample.cloudformation.yaml")).unwrap();
        actual += "\nsample.cloudformation.yaml\n";
        let files = [("stack/template.yaml".to_string(), src.clone())];
        actual += &format!("{:#?}\n", crate::iac::evaluate(&files));
        let (_, rows) = config_values::read("stack/template.yaml", &src);
        actual.extend(rows.iter().map(|row| format!("{row:?}\n")));
        actual += "\ncontracts.role\n";
        actual.extend(
            crate::contract::Role::ALL
                .iter()
                .map(|role| format!("{}\n", role.name())),
        );
        actual += "\nunfollowed.method\n";
        actual.extend(
            (crate::http::IncludeReason::ALL.iter()).map(|reason| format!("{}\n", reason.name())),
        );

        let src = fs::read(tests.join("fixtures/schema/sample.serverless.yml")).unwrap();
        actual += "\nsample.serverless.yml\n";
        let files = [("api/serverless.yml".to_string(), src)];
        actual += &format!("{:#?}\n", crate::iac::evaluate(&files));

        let src = fs::read(tests.join("fixtures/schema/sample.provision.tf")).unwrap();
        actual += "\nsample.provision.tf\n";
        actual += &format!("{:#?}\n", crate::iac::evaluate(&[("main.tf".into(), src)]));

        let src = fs::read(tests.join("fixtures/schema/sample.compose.yaml")).unwrap();
        actual += "\nsample.compose.yaml\n";
        let files = [("compose.yaml".to_string(), src.clone())];
        let (nodes, references) = crate::iac::evaluate(&files);
        actual += &format!("{:#?}\n", (&nodes, &references));
        let nodes: Vec<_> = nodes.iter().collect();
        actual += &serde_json::to_string_pretty(&crate::contract::config_summary(
            "compose.yaml",
            &src,
            &tests.join("fixtures/schema"),
            &nodes,
            &[],
        ))
        .unwrap();
        actual += "\n";

        for (name, path) in [
            ("sample.workflow.yml", ".github/workflows/sample.yml"),
            ("sample.action.yml", "tools/setup/action.yml"),
        ] {
            let src = fs::read(tests.join("fixtures/schema").join(name)).unwrap();
            let evaluated = crate::iac::actions::evaluate(path, &src);
            actual += &format!("\n{name}\n{evaluated:#?}\n");
        }

        let src = fs::read(tests.join("fixtures/schema/sample.alert.yaml")).unwrap();
        actual += "\nsample.alert.yaml\n";
        actual += &serde_json::to_string_pretty(&crate::contract::config_summary(
            "sample.alert.yaml",
            &src,
            &tests.join("fixtures/schema"),
            &[],
            &[],
        ))
        .unwrap();
        actual += "\n";

        let src = fs::read(tests.join("fixtures/schema/sample.incident.tf")).unwrap();
        actual += "\nsample.incident.tf\n";
        actual += &serde_json::to_string_pretty(&crate::contract::config_summary(
            "sample.incident.tf",
            &src,
            &tests.join("fixtures/schema"),
            &[],
            &[],
        ))
        .unwrap();
        actual += "\n";

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
            refresh(&db, &root, REPO, &|| Ok(serve.clone())).unwrap();
        };
        let is_fresh = |identity: &str| {
            let db = read(&index).unwrap();
            fresh(&db, &root, identity, &serve).unwrap()
        };
        index_now();
        let before = tree(&dir);
        assert!(is_fresh(REPO));
        assert!(!is_fresh("example.com/acme/other"));
        assert_eq!(tree(&dir), before, "fresh wrote nothing");

        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        let a = fs::File::options()
            .write(true)
            .open(root.join("a.ts"))
            .unwrap();
        a.set_modified(later).unwrap();
        assert!(is_fresh(REPO));

        let sam = "Transform: AWS::Serverless-2016-10-31\nResources:\n  Worker:\n    Type: \
                   AWS::Serverless::Function\n    Properties:\n      CodeUri: worker.zip\n      \
                   Handler: index.handler\n      Runtime: nodejs20.x\n";
        let handler = sam.replace("worker.zip", "worker/");
        let edits: [(&str, Option<&str>); 20] = [
            ("a.ts", Some("export function b() {}\n")),
            ("b.ts", Some("export const c = 1;\n")),
            ("b.ts", None),
            ("package.json", Some("{\"name\": \"site\"}\n")),
            (
                "main.tofu",
                Some("variable \"x\" {\n  default = \"d\"\n}\n"),
            ),
            ("override.tf", Some("locals {}\n")),
            ("main.tf", Some("variable \"x\" {}\n")),
            ("main.tf", None),
            ("template.yaml", Some(sam)),
            ("worker.zip", Some("PK")),
            ("worker.zip", None),
            ("template.yaml", Some(&handler)),
            ("worker/index.js", Some("exports.handler = () => 1;\n")),
            ("worker/index.js", None),
            (
                "compose.yaml",
                Some("services:\n  api:\n    build: ./app\n"),
            ),
            ("app/", Some("")),
            ("app/Dockerfile", Some("FROM x\nCOPY . .\n")),
            ("app/Dockerfile", Some("FROM x\nCOPY src .\n")),
            ("app/Dockerfile", None),
            ("app/", None),
        ];
        for (file, content) in edits {
            match (file.strip_suffix('/'), content) {
                (Some(dir), Some(_)) => fs::create_dir_all(root.join(dir)).unwrap(),
                (Some(dir), None) => fs::remove_dir_all(root.join(dir)).unwrap(),
                (None, Some(content)) => {
                    fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
                    fs::write(root.join(file), content).unwrap();
                }
                (None, None) => fs::remove_file(root.join(file)).unwrap(),
            }
            assert!(!is_fresh(REPO), "{file} {content:?}");
            index_now();
            assert!(is_fresh(REPO), "{file} {content:?} indexed");
        }
        let _ = fs::remove_dir_all(&dir);
    }

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
            refresh(&db, &root, REPO, &|| Ok(serve.clone())).unwrap();
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

    fn past_the_ceiling(indent: &str) -> String {
        (0..=config_values::VALUES_MAX)
            .map(|i| format!("{indent}p{i}: {i}\n"))
            .collect()
    }

    fn config_rows(db: &Connection, path: &str) -> (i64, config_values::Outcome) {
        let rows = "SELECT count(*) FROM config_values WHERE path = ?1";
        let rows = db.query_row(rows, [path], |r| r.get(0)).unwrap();
        let summary = "SELECT summary FROM config_files WHERE path = ?1";
        let summary: String = db.query_row(summary, [path], |r| r.get(0)).unwrap();
        (rows, serde_json::from_str(&summary).unwrap())
    }

    #[test]
    fn a_file_of_a_kind_keeps_its_values_past_the_ceiling_and_a_generic_one_none() {
        let (dir, index) = index_dir("kind_files_past_the_ceiling");
        let root = checkout(&dir);
        let pad = past_the_ceiling("  ");
        let json_pad: Vec<String> = (0..=config_values::VALUES_MAX)
            .map(|i| format!("\"p{i}\": {i}"))
            .collect();
        let json_pad = json_pad.join(", ");
        let files = [
            (
                "infra/main.tf.json",
                format!(
                    "{{\"resource\": {{\"aws_s3_bucket\": {{\"b\": {{\"bucket\": \"acme\"}}}}}}, \
                     \"locals\": {{{json_pad}}}}}"
                ),
            ),
            ("infra/prod.tfvars.json", format!("{{{json_pad}}}")),
            (
                "docker-compose.yml",
                format!("services:\n  web:\n    image: acme/web:1\nx-pad:\n{pad}"),
            ),
            (
                "stack/template.yaml",
                format!(
                    "AWSTemplateFormatVersion: '2010-09-09'\nResources:\n  Queue:\n    \
                     Type: AWS::SQS::Queue\nMetadata:\n{pad}"
                ),
            ),
            (
                "serverless.yml",
                format!(
                    "service: api\nprovider:\n  name: aws\nfunctions:\n  hello:\n    \
                     handler: handler.hello\ncustom:\n{pad}"
                ),
            ),
            (
                "k8s/web.yaml",
                format!(
                    "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n  \
                     annotations:\n{}spec:\n  template:\n    spec:\n      containers:\n      \
                     - name: web\n        image: acme/web:1\n",
                    past_the_ceiling("    ")
                ),
            ),
            (
                "rules/alerts.yaml",
                format!(
                    "groups:\n- name: g\n  rules:\n  - alert: Down\n    expr: up == 0\nx-pad:\n{pad}"
                ),
            ),
            (
                ".github/workflows/ci.yml",
                format!(
                    "on: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n    \
                     - uses: docker/build-push-action@v6\n      with:\n        tags: \
                     acme/api:1\nenv:\n{pad}"
                ),
            ),
            (
                "skaffold.yaml",
                format!(
                    "apiVersion: skaffold/v4beta6\nkind: Config\nbuild:\n  artifacts:\n  - \
                     image: acme/web\nx-pad:\n{pad}"
                ),
            ),
            (
                "openapi.yaml",
                format!(
                    "openapi: 3.0.0\ninfo:\n  title: api\n  version: '1'\npaths:\n  /orders:\n    \
                     get:\n      responses:\n        '200':\n          description: ok\nx-pad:\n{pad}"
                ),
            ),
            (
                "package.json",
                format!("{{\"name\": \"web\", \"version\": \"1.0.0\", \"x-pad\": {{{json_pad}}}}}"),
            ),
            (
                "tsconfig.json",
                format!("{{\"compilerOptions\": {{\"strict\": true}}, \"x-pad\": {{{json_pad}}}}}"),
            ),
            (
                "app-config.yaml",
                format!("app:\n  title: web\nx-pad:\n{pad}"),
            ),
            (
                "asyncapi.yaml",
                format!(
                    "asyncapi: 2.6.0\ninfo:\n  title: events\n  version: '1'\nchannels:\n  \
                     orders: {{}}\nx-pad:\n{pad}"
                ),
            ),
            (
                "alertmanager.yml",
                format!("route:\n  receiver: team\nreceivers:\n- name: team\nx-pad:\n{pad}"),
            ),
            (
                "k8s/prod/kustomization.yaml",
                format!("resources:\n- ../web.yaml\nnamePrefix: prod-\nx-pad:\n{pad}"),
            ),
            (
                "src/main/resources/application.yml",
                format!("stock:\n  url: https://web.example.com\nx-pad:\n{pad}"),
            ),
            (
                "appsettings.json",
                format!(
                    "{{\"Api\": {{\"Url\": \"https://config.example.com/v2\"}}, \
                     \"x-pad\": {{{json_pad}}}}}"
                ),
            ),
            ("data/measure.json", format!("{{{json_pad}}}")),
            (
                "chart/values.yaml",
                format!("image: acme/web:1\nx-pad:\n{pad}"),
            ),
        ];
        let code = [
            (
                "src/main/java/com/example/api/StockClient.java",
                "package com.example.api;\nimport org.springframework.beans.factory.annotation.Value;\n\
                 import org.springframework.web.reactive.function.client.WebClient;\n\
                 public class StockClient {\n    private final WebClient web;\n    \
                 public StockClient(@Value(\"${stock.url}\") String base) {\n        \
                 this.web = WebClient.create(base);\n    }\n    \
                 public void level() {\n        web.get().uri(\"/api/stock\").retrieve();\n    }\n}\n",
            ),
            (
                "A/IApi.cs",
                "using Refit;\nusing System.Threading.Tasks;\nnamespace A;\n\
                 public interface IApi {\n[Get(\"/items/{id}\")] Task<string> Item(int id);\n}\n",
            ),
            (
                "A/Make.cs",
                "using Refit;\nusing Microsoft.Extensions.Configuration;\nnamespace A;\n\
                 public class Make {\n\
                 public IApi Api(IConfiguration config) => RestService.For<IApi>(config[\"Api:Url\"]);\n}\n",
            ),
        ];
        let written = (files.iter()).map(|(path, text)| (*path, text.as_str()));
        for (path, text) in written.chain(code) {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let serve = Serve {
            declared: true,
            ..Serve::default()
        };
        let db = open(&index, &root, REPO, &serve).unwrap();

        let values = config_values::VALUES_MAX as i64 + 1;
        for (path, _) in &files[..files.len() - 2] {
            let (rows, outcome) = config_rows(&db, path);
            assert!(rows >= values, "{path}: {rows} rows, {outcome:?}");
            assert!(
                matches!(outcome, config_values::Outcome::Read { values: v, .. } if v as i64 == rows),
                "{path}: {outcome:?}"
            );
        }
        for path in ["data/measure.json", "chart/values.yaml"] {
            assert_eq!(
                config_rows(&db, path),
                (0, config_values::Outcome::Data),
                "{path}"
            );
        }
        let count =
            |sql: &str, path: &str| -> i64 { db.query_row(sql, [path], |r| r.get(0)).unwrap() };
        let nodes = "SELECT count(*) FROM iac_nodes WHERE path = ?1";
        for path in [
            "infra/main.tf.json",
            "docker-compose.yml",
            "stack/template.yaml",
            "serverless.yml",
            "k8s/web.yaml",
        ] {
            assert!(count(nodes, path) > 0, "{path}: no IaC node");
        }
        for path in ["openapi.yaml", "asyncapi.yaml"] {
            assert_eq!(
                count("SELECT count(*) FROM documents WHERE path = ?1", path),
                1,
                "{path}"
            );
        }
        let overlay = "SELECT count(*) FROM iac_nodes WHERE address LIKE ?1";
        assert!(count(overlay, "%prod-web%") > 0, "no object of the overlay");
        let client = "SELECT count(*) FROM contracts WHERE contract = ?1 AND via LIKE ?2";
        let clients = |contract: &str, via: &str| -> i64 {
            db.query_row(client, [contract, via], |r| r.get(0)).unwrap()
        };
        assert_eq!(clients("http::GET::/api/stock", "host:web.example.com"), 1);
        assert!(clients("http::GET::/v2/items/{}", "%host:config.example.com%") > 0);
        for path in ["package.json", "tsconfig.json"] {
            assert_eq!(
                count("SELECT count(*) FROM manifests WHERE path = ?1", path),
                1,
                "{path}"
            );
        }
        let summary = |path: &str| -> String {
            let sql = "SELECT summary FROM contract_configs WHERE path = ?1";
            db.query_row(sql, [path], |r| r.get(0)).unwrap()
        };
        assert!(summary("rules/alerts.yaml").contains("Down"));
        assert!(summary("alertmanager.yml").contains("team"));
        assert!(summary(".github/workflows/ci.yml").contains("acme/api"));
        assert!(summary("skaffold.yaml").contains("acme/web"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_data_file_keeps_its_values_while_a_serve_table_lists_it() {
        let (dir, index) = index_dir("data_file_listed_later");
        let root = checkout(&dir);
        let spec = format!("openapi: 3.0.0\nx-pad:\n{}", past_the_ceiling("  "));
        fs::write(root.join("spec.yaml"), spec).unwrap();
        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        assert_eq!(
            config_rows(&db, "spec.yaml"),
            (0, config_values::Outcome::Data)
        );
        let listed = Serve {
            declared: true,
            documents: vec!["spec.yaml".into()],
            ..Serve::default()
        };
        super::refresh(&db, &root, REPO, &|| Ok(listed.clone())).unwrap();
        let (rows, outcome) = config_rows(&db, "spec.yaml");
        assert_eq!(rows, config_values::VALUES_MAX as i64 + 2, "{outcome:?}");
        super::refresh(&db, &root, REPO, &|| Ok(Serve::default())).unwrap();
        assert_eq!(
            config_rows(&db, "spec.yaml"),
            (0, config_values::Outcome::Data)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_index_of_another_version_drops_the_rows_of_a_data_file() {
        let (dir, index) = index_dir("data_rows_of_another_version");
        let root = checkout(&dir);
        fs::write(root.join("measure.yaml"), past_the_ceiling("")).unwrap();
        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        assert_eq!(
            config_rows(&db, "measure.yaml"),
            (0, config_values::Outcome::Data)
        );
        let rows: Vec<config_values::Row> = (0..=config_values::VALUES_MAX)
            .map(|i| config_values::Row {
                key: format!("p{i}"),
                value: Some(i.to_string()),
                kind: "number",
                line: i + 1,
                cut: false,
            })
            .collect();
        let tx = rusqlite::Transaction::new_unchecked(&db, rusqlite::TransactionBehavior::Deferred)
            .unwrap();
        config_values::store(&tx, "measure.yaml", &rows).unwrap();
        let read = config_values::Outcome::Read {
            values: rows.len(),
            redacted: 0,
            sensitive: vec![],
        };
        let read = serde_json::to_string(&read).unwrap();
        tx.execute(
            "UPDATE config_files SET summary = ?1 WHERE path = 'measure.yaml'",
            [read],
        )
        .unwrap();
        tx.commit().unwrap();
        db.pragma_update(
            None,
            "user_version",
            super::schema_version().wrapping_add(1),
        )
        .unwrap();
        drop(db);

        let db = open(&index, &root, REPO, &Serve::default()).unwrap();
        assert_eq!(
            config_rows(&db, "measure.yaml"),
            (0, config_values::Outcome::Data)
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
