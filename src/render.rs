//! Compact answers: line-oriented text under a token budget (ADR 0008), whose helpers live in
//! `meada_core::render`; here, the source files an answer reads to place a line.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path};

pub(crate) use meada_core::render::*;

/// Source files read once per answer.
pub(crate) struct Files<'a> {
    root: &'a Path,
    read: HashMap<String, Option<Vec<u8>>>,
}

impl<'a> Files<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self {
            root,
            read: HashMap::new(),
        }
    }

    pub(crate) fn get(&mut self, path: &str) -> Option<&[u8]> {
        self.read
            .entry(path.to_string())
            .or_insert_with(|| source(self.root, path))
            .as_deref()
    }

    /// `path:line`; just `path` when the file cannot be read. The line is display only: it
    /// comes from the byte offset and the file as it is now.
    pub(crate) fn at(&mut self, path: &str, offset: i64) -> String {
        match self.get(path) {
            Some(src) => format!("{path}:{}", line_of(src, offset)),
            None => path.to_string(),
        }
    }
}

// ponytail: a source file an answer reads past this renders as unavailable; indexing reads a
// source file whole, and an answer keeps each file it reads, so several large ones add up.
pub(crate) const SOURCE_MAX: u64 = 16 << 20;

/// The source file at `path` under `root`, at most `SOURCE_MAX` bytes, or `None` when it cannot
/// be read so. `path` comes from an index row, hostile in a pulled generation (ADR 0012): only
/// plain parts are joined (no root, no `..`), no part is a symbolic link and the file is a
/// regular one, so an absolute path, a FIFO or `/dev/zero` is refused before it is opened. The
/// walk follows no symbolic link, so no indexed path goes through one.
pub(crate) fn source(root: &Path, path: &str) -> Option<Vec<u8>> {
    let mut abs = root.to_path_buf();
    for part in Path::new(path).components() {
        let Component::Normal(part) = part else {
            return None;
        };
        abs.push(part);
        if fs::symlink_metadata(&abs).ok()?.file_type().is_symlink() {
            return None;
        }
    }
    if !fs::symlink_metadata(&abs).ok()?.is_file() {
        return None;
    }
    let mut src = Vec::new();
    (fs::File::open(&abs).ok()?)
        .take(SOURCE_MAX + 1)
        .read_to_end(&mut src)
        .ok()?;
    (src.len() as u64 <= SOURCE_MAX).then_some(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(case: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("meada-render-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("root/src")).unwrap();
        fs::write(dir.join("root/src/a.ts"), "one\ntwo\n").unwrap();
        fs::write(dir.join("outside.ts"), "secret\nlines\n").unwrap();
        dir
    }

    #[test]
    fn a_source_inside_the_root_is_read() {
        let dir = scratch("inside");
        let root = dir.join("root");
        assert_eq!(
            source(&root, "src/a.ts").as_deref(),
            Some(&b"one\ntwo\n"[..])
        );
        assert_eq!(Files::new(&root).at("src/a.ts", 4), "src/a.ts:2");
    }

    #[test]
    fn an_absolute_or_escaping_source_path_is_unavailable() {
        let dir = scratch("escape");
        let root = dir.join("root");
        let outside = dir.join("outside.ts").display().to_string();
        for path in [
            outside.as_str(),
            "../outside.ts",
            "src/../../outside.ts",
            "/dev/zero",
            "",
        ] {
            assert_eq!(source(&root, path), None, "{path}");
            assert_eq!(Files::new(&root).at(path, 8), path, "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_source_through_a_symbolic_link_is_unavailable() {
        let dir = scratch("link");
        let root = dir.join("root");
        std::os::unix::fs::symlink(dir.join("outside.ts"), root.join("src/l.ts")).unwrap();
        std::os::unix::fs::symlink(&dir, root.join("up")).unwrap();
        assert_eq!(source(&root, "src/l.ts"), None);
        assert_eq!(source(&root, "up/outside.ts"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_source_is_unavailable_without_blocking() {
        let dir = scratch("fifo");
        let root = dir.join("root");
        let made = std::process::Command::new("mkfifo")
            .arg(root.join("src/f.ts"))
            .status()
            .unwrap();
        assert!(made.success());
        assert_eq!(source(&root, "src/f.ts"), None);
    }

    #[test]
    fn a_source_past_the_ceiling_is_unavailable() {
        let dir = scratch("large");
        let root = dir.join("root");
        let file = fs::File::create(root.join("src/big.ts")).unwrap();
        file.set_len(SOURCE_MAX + 1).unwrap();
        assert_eq!(source(&root, "src/big.ts"), None);
        fs::File::options()
            .write(true)
            .open(root.join("src/big.ts"))
            .unwrap()
            .set_len(SOURCE_MAX)
            .unwrap();
        assert_eq!(
            source(&root, "src/big.ts").map(|s| s.len() as u64),
            Some(SOURCE_MAX)
        );
    }
}
