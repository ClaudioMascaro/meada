//! The values cache (ADR 0027): beside the index, in Meada's own directory, what a pack parsed
//! of each file the last contract extraction read the values of (`valued`), whole, so that a
//! refresh reads those values back instead of parsing the file again. An entry is keyed by the
//! file's bytes, the setting it is parsed under, its pack and the build of Meada (`key`); a
//! parse gives the same facts for the same key, so a hit is what a parse would give. The cache
//! holds one entry per file the index lists in `valued`, pruned on every extraction. Anything
//! wrong with it (missing, unreadable, an entry that does not deserialize) is a miss: it costs a
//! parse, never a different index.

use std::cell::{OnceCell, RefCell};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, params};

use crate::lang::{self, Facts};

/// The cache's file, beside the index's.
pub(crate) const FILE: &str = "values.db";

/// The largest entry kept, as JSON. A pack's limits bound what it parses, not what that
/// serializes to: a C# read's scope is one string shared by every read of a node in memory
/// and written out once per read.
// ponytail: a flat 8 MiB, five times this repository's largest entry (1.6 MB); a file past it is
// parsed on every refresh, which only a crafted file pays.
pub(crate) const ENTRY_BYTES: usize = 8 << 20;

/// One row per file: its path, the key it was parsed under (`key`) and its facts as JSON.
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS entries \
    (path TEXT PRIMARY KEY, key TEXT NOT NULL, facts BLOB NOT NULL);";

/// The values cache of one refresh: what it read, and what it parsed to write back.
pub(crate) struct Values {
    /// The cache's file, beside the index's; none for an index in memory.
    at: Option<PathBuf>,
    /// The cache opened on the first read, `None` when it is missing or cannot be read.
    db: OnceCell<Option<Connection>>,
    /// The entries parsed on this refresh: path, key and facts as JSON.
    fresh: RefCell<Vec<(String, String, Vec<u8>)>>,
}

impl Values {
    /// The cache beside the index `index` holds open. Nothing is opened or created until a
    /// file is read or written. An index in memory, or a build that cannot tell itself from
    /// another (`build`), has none.
    pub(crate) fn beside(index: &Connection) -> Values {
        let at = (index.path().filter(|p| !p.is_empty()))
            .filter(|_| build().is_some())
            .and_then(|p| Path::new(p).parent().map(|dir| dir.join(FILE)));
        Values {
            at,
            db: OnceCell::new(),
            fresh: RefCell::default(),
        }
    }

    /// The cache opened to read on first use, never created: a missing one reads as empty.
    fn db(&self) -> Option<&Connection> {
        let at = self.at.as_deref()?;
        (self.db.get_or_init(|| connect(at, false).ok())).as_ref()
    }

    /// The facts stored for `path` under `key`, as its pack parsed them.
    pub(crate) fn get(&self, path: &str, key: &str) -> Option<Facts> {
        let db = self.db()?;
        let mut statement =
            (db.prepare_cached("SELECT facts FROM entries WHERE path = ?1 AND key = ?2")).ok()?;
        let blob: Vec<u8> = (statement.query_row(params![path, key], |r| r.get(0)))
            .optional()
            .ok()??;
        // ponytail: an entry nested past `serde_json`'s depth (128) does not deserialize, so its
        // file is parsed on every refresh; a deeper limit needs a deserializer without one.
        serde_json::from_slice(&blob).ok()
    }

    /// Keeps `facts`, as `path`'s pack parsed it under `key`, to write back (`keep`).
    pub(crate) fn put(&self, path: &str, key: String, facts: &Facts) {
        if self.at.is_none() {
            return;
        }
        if let Some(json) = entry(facts) {
            self.fresh.borrow_mut().push((path.to_string(), key, json));
        }
    }

    /// Writes the entries parsed on this refresh and drops those of every file not in `read`
    /// (the new `valued`): the cache never holds more than one entry per file the extraction
    /// reads. A cache missing or unreadable is created again when there is an entry to write.
    /// A write that fails leaves the cache as it was, which costs only time.
    pub(crate) fn keep(self, read: &BTreeSet<String>) {
        let Some(at) = self.at else { return };
        let fresh = self.fresh.into_inner();
        let db = match self.db.into_inner().flatten() {
            Some(db) => Some(db),
            // Nothing read: still pruned, since `read` may have lost files.
            None if fresh.is_empty() => connect(&at, false).ok(),
            None => match connect(&at, true) {
                Err(e) if !busy(&e) => {
                    let _ = fs::remove_file(&at);
                    connect(&at, true).ok()
                }
                db => db.ok(),
            },
        };
        let Some(mut db) = db else { return };
        let _ = (|| -> rusqlite::Result<()> {
            let stale: Vec<String> = (db.prepare("SELECT path FROM entries")?)
                .query_map([], |r| r.get::<_, String>(0))?
                .filter(|path| path.as_ref().map_or(true, |p| !read.contains(p)))
                .collect::<rusqlite::Result<_>>()?;
            if stale.is_empty() && fresh.is_empty() {
                return Ok(());
            }
            let tx = db.transaction()?;
            for path in &stale {
                tx.execute("DELETE FROM entries WHERE path = ?1", params![path])?;
            }
            for (path, key, facts) in fresh.iter().filter(|(path, ..)| read.contains(path)) {
                tx.execute(
                    "INSERT OR REPLACE INTO entries VALUES (?1, ?2, ?3)",
                    params![path, key, facts],
                )?;
            }
            tx.commit()
        })();
    }
}

/// `facts` as an entry: JSON with its values, `None` past `ENTRY_BYTES`, which the
/// serialization stops at rather than reach.
fn entry(facts: &Facts) -> Option<Vec<u8>> {
    struct Capped(Vec<u8>);
    impl std::io::Write for Capped {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.0.len() + buf.len() > ENTRY_BYTES {
                return Err(std::io::Error::other("past ENTRY_BYTES"));
            }
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Capped(Vec::new());
    lang::with_values(|| serde_json::to_writer(&mut out, facts)).ok()?;
    Some(out.0)
}

/// The cache at `at`, created when missing if `create` says so.
fn connect(at: &Path, create: bool) -> rusqlite::Result<Connection> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let db = Connection::open_with_flags(at, flags)?;
    db.busy_timeout(super::BUSY.get())?;
    db.execute_batch(SCHEMA)?;
    // A table of another shape (another build's) is no cache of this one.
    db.prepare("SELECT path, key, facts FROM entries LIMIT 0")?;
    Ok(db)
}

/// Whether `e` is another connection's lock held past the busy timeout: no reason to recreate.
fn busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// The key of the facts `pack` parses of `src` under `setting`: those and the build of Meada,
/// which is its version, the stored format's and the running executable's stat, so that two
/// builds of one version (a working tree, an installed binary) never read each other's values.
pub(crate) fn key(pack: usize, setting: &str, src: &[u8]) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(build().unwrap_or_default().as_bytes());
    hash.update(&(pack as u64).to_le_bytes());
    hash.update(&(setting.len() as u64).to_le_bytes());
    hash.update(setting.as_bytes());
    hash.update(src);
    hash.finalize().to_hex().to_string()
}

/// The build of Meada running: its version, the stored format's and the executable's path,
/// size and mtime. `None` when the executable cannot be found or read, so a build that cannot
/// tell itself from another of its version keeps no cache.
fn build() -> Option<&'static str> {
    static BUILD: OnceLock<Option<String>> = OnceLock::new();
    let build = BUILD.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        let stat = fs::metadata(&exe).ok()?;
        let modified = stat
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(format!(
            "{}\n{}\n{}\n{}\n{}",
            env!("CARGO_PKG_VERSION"),
            super::schema_version(),
            exe.display(),
            stat.len(),
            modified.as_nanos(),
        ))
    });
    build.as_deref()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::lang::{self, Facts, Part, Value};

    /// An entry past `ENTRY_BYTES` is not kept (its file is parsed again instead); one within it is.
    #[test]
    fn an_entry_past_its_size_limit_is_not_kept() {
        let mut facts = Facts::default();
        let text = |n| Value::Str(vec![Part::Text("a".repeat(n))]);
        facts
            .rebound
            .push(("x".into(), text(super::ENTRY_BYTES / 2)));
        assert!(super::entry(&facts).is_some());
        facts
            .rebound
            .push(("y".into(), text(super::ENTRY_BYTES / 2)));
        assert!(super::entry(&facts).is_none());
    }

    fn files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            match path.is_dir() {
                true => files(&path, out),
                false => out.push(path),
            }
        }
    }

    /// An entry deserializes to the facts its pack parsed, values and all, for every file of
    /// every fixture: a hit is what a parse gives. Outside `with_values` the facts serialize
    /// as the index keeps them, without the values.
    #[test]
    fn every_fixture_file_round_trips_through_an_entry_whole() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut all = Vec::new();
        files(&root, &mut all);
        let (mut parsed, mut valued) = (0, 0);
        for file in all {
            let path = file
                .strip_prefix(&root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let Some(pack) = lang::pack_for(&path) else {
                continue;
            };
            let src = fs::read(&file).unwrap();
            let Ok(facts) = lang::pack(pack).parse(&path, &src) else {
                continue;
            };
            parsed += 1;
            let whole = lang::with_values(|| serde_json::to_value(&facts)).unwrap();
            let json = lang::with_values(|| serde_json::to_vec(&facts)).unwrap();
            let back: Facts = serde_json::from_slice(&json).unwrap();
            let again = lang::with_values(|| serde_json::to_value(&back)).unwrap();
            assert_eq!(whole, again, "{path}");
            let lean = serde_json::to_value(&facts).unwrap();
            assert_eq!(lean, serde_json::to_value(&back).unwrap(), "{path}");
            valued += usize::from(lean != whole);
        }
        assert!(
            parsed > 500 && valued > 100,
            "{parsed} parsed, {valued} with values"
        );
    }
}
