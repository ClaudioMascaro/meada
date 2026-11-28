use std::cell::{OnceCell, RefCell};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, params};

use crate::lang::{self, Facts};

pub(crate) const FILE: &str = "values.db";

// ponytail: a flat 8 MiB, five times this repository's largest entry (1.6 MB); a file past it is
// parsed on every refresh, which only a crafted file pays.
pub(crate) const ENTRY_BYTES: usize = 8 << 20;

const LEVEL: u8 = 6;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS entries \
    (path TEXT PRIMARY KEY, key TEXT NOT NULL, facts BLOB NOT NULL);";

pub(crate) struct Values {
    at: Option<PathBuf>,
    db: OnceCell<Option<Connection>>,
    fresh: RefCell<Vec<(String, String, Vec<u8>)>>,
}

impl Values {
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

    fn db(&self) -> Option<&Connection> {
        let at = self.at.as_deref()?;
        (self.db.get_or_init(|| connect(at, false).ok())).as_ref()
    }

    pub(crate) fn get(&self, path: &str, key: &str) -> Option<Facts> {
        let db = self.db()?;
        let mut statement =
            (db.prepare_cached("SELECT facts FROM entries WHERE path = ?1 AND key = ?2")).ok()?;
        let blob: Vec<u8> = (statement.query_row(params![path, key], |r| r.get(0)))
            .optional()
            .ok()??;
        facts_of(&blob)
    }

    pub(crate) fn put(&self, path: &str, key: String, facts: &Facts) {
        if self.at.is_none() {
            return;
        }
        if let Some(json) = entry(facts) {
            self.fresh.borrow_mut().push((path.to_string(), key, json));
        }
    }

    pub(crate) fn keep(self, read: &BTreeSet<String>) {
        let Some(at) = self.at else { return };
        let fresh = self.fresh.into_inner();
        let db = match self.db.into_inner().flatten() {
            Some(db) => Some(db),
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
            tx.commit()?;
            let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
            let free: i64 = db.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
            if free * 2 > pages {
                db.execute_batch("VACUUM")?;
            }
            Ok(())
        })();
    }
}

fn facts_of(blob: &[u8]) -> Option<Facts> {
    use miniz_oxide::inflate::TINFLStatus;
    use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};
    let flags = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
        | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let mut state = Box::<DecompressorOxide>::default();
    let mut json = vec![0; blob.len().saturating_mul(2).clamp(64, ENTRY_BYTES)];
    let (mut rest, mut len) = (blob, 0);
    loop {
        let (status, read, wrote) = decompress(&mut state, rest, &mut json, len, flags);
        len += wrote;
        rest = rest.get(read..)?;
        match status {
            TINFLStatus::Done if rest.is_empty() => break,
            TINFLStatus::HasMoreOutput if json.len() < ENTRY_BYTES => {
                json.resize(json.len().saturating_mul(2).min(ENTRY_BYTES), 0);
            }
            _ => return None,
        }
    }
    json.truncate(len);
    // ponytail: an entry nested past `serde_json`'s depth (128) does not deserialize, so its
    // file is parsed on every refresh; a deeper limit needs a deserializer without one.
    serde_json::from_slice(&json).ok()
}

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
    Some(miniz_oxide::deflate::compress_to_vec_zlib(&out.0, LEVEL))
}

fn connect(at: &Path, create: bool) -> rusqlite::Result<Connection> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let db = Connection::open_with_flags(at, flags)?;
    db.busy_timeout(super::BUSY.get())?;
    db.execute_batch(SCHEMA)?;
    db.prepare("SELECT path, key, facts FROM entries LIMIT 0")?;
    Ok(db)
}

fn busy(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

pub(crate) fn key(pack: usize, setting: &str, src: &[u8]) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(build().unwrap_or_default().as_bytes());
    hash.update(&(pack as u64).to_le_bytes());
    hash.update(&(setting.len() as u64).to_le_bytes());
    hash.update(setting.as_bytes());
    hash.update(src);
    hash.finalize().to_hex().to_string()
}

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
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    use crate::lang::{self, Facts, Part, Value};

    fn noise(seed: u64, n: usize) -> Vec<u8> {
        let mut out = vec![0; n];
        (blake3::Hasher::new().update(&seed.to_le_bytes()))
            .finalize_xof()
            .fill(&mut out);
        out
    }

    #[test]
    fn a_blob_that_inflates_past_its_size_limit_or_is_no_entry_is_a_miss() {
        let json = |n: usize| {
            let mut json = serde_json::to_vec(&Facts::default()).unwrap();
            json.resize(n, b' ');
            json
        };
        let deflate = |json: &[u8]| miniz_oxide::deflate::compress_to_vec_zlib(json, super::LEVEL);
        assert!(super::facts_of(&deflate(&json(super::ENTRY_BYTES))).is_some());
        let past = deflate(&json(super::ENTRY_BYTES + 1));
        assert!(past.len() < 64 << 10, "{} bytes", past.len());
        assert!(super::facts_of(&past).is_none());
        let plain = serde_json::to_vec(&Facts::default()).unwrap();
        assert!(serde_json::from_slice::<Facts>(&plain).is_ok());
        assert!(super::facts_of(&plain).is_none());
        assert!(super::facts_of(&noise(7, 4096)).is_none());
        let whole = deflate(&plain);
        assert!(super::facts_of(&whole).is_some());
        for tail in [&whole[..], &b"garbage"[..]] {
            assert!(super::facts_of(&[&whole[..], tail].concat()).is_none());
        }
    }

    #[test]
    fn entries_replaced_by_smaller_ones_give_their_pages_back() {
        let dir = std::env::temp_dir().join(format!("meada-values-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let index = rusqlite::Connection::open(dir.join("index.db")).unwrap();
        let paths: BTreeSet<String> = (0..20).map(|n| format!("src/f{n}.rs")).collect();
        let write = |bytes: usize| {
            let values = super::Values::beside(&index);
            for (seed, path) in (0..).zip(&paths) {
                let text: String = (noise(seed, bytes / 2).iter())
                    .map(|b| format!("{b:02x}"))
                    .collect();
                let mut facts = Facts::default();
                facts
                    .rebound
                    .push(("x".into(), Value::Str(vec![Part::Text(text)])));
                values.put(path, format!("{path} {bytes}"), &facts);
            }
            values.keep(&paths);
            fs::metadata(dir.join(super::FILE)).unwrap().len()
        };
        let before = write(64 << 10);
        let after = write((64 << 10) / 10);
        assert!(after * 2 < before, "{before} bytes, then {after}");
        drop(index);
        fs::remove_dir_all(&dir).unwrap();
    }

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
            let back = super::facts_of(&super::entry(&facts).unwrap()).unwrap();
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
