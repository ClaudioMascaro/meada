use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path};

pub(crate) use meada_core::render::*;

pub(crate) struct Files<'a> {
    root: &'a Path,
    read: HashMap<String, Option<Source>>,
}

const LINE_BLOCK: usize = 1024;

struct Source {
    bytes: Vec<u8>,
    lines: Vec<usize>,
}

impl Source {
    fn new(bytes: Vec<u8>) -> Self {
        let counted = bytes.chunks(LINE_BLOCK).scan(0, |count, block| {
            *count += newlines(block);
            Some(*count)
        });
        let lines = std::iter::once(0).chain(counted).collect();
        Self { bytes, lines }
    }

    fn line_of(&self, offset: i64) -> usize {
        let end = (offset.max(0) as usize).min(self.bytes.len());
        let block = end / LINE_BLOCK;
        self.lines[block] + newlines(&self.bytes[block * LINE_BLOCK..end]) + 1
    }
}

fn newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count()
}

impl<'a> Files<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self {
            root,
            read: HashMap::new(),
        }
    }

    fn source(&mut self, path: &str) -> Option<&Source> {
        self.read
            .entry(path.to_string())
            .or_insert_with(|| source(self.root, path).map(Source::new))
            .as_ref()
    }

    pub(crate) fn get(&mut self, path: &str) -> Option<&[u8]> {
        self.source(path).map(|s| s.bytes.as_slice())
    }

    pub(crate) fn line(&mut self, path: &str, offset: i64) -> Option<usize> {
        self.source(path).map(|s| s.line_of(offset))
    }

    pub(crate) fn at(&mut self, path: &str, offset: i64) -> String {
        match self.line(path, offset) {
            Some(line) => format!("{path}:{line}"),
            None => path.to_string(),
        }
    }
}

// ponytail: a source file an answer reads past this renders as unavailable; indexing reads a
// source file whole, and an answer keeps each file it reads, so several large ones add up.
pub(crate) const SOURCE_MAX: u64 = 16 << 20;

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
    fn an_offset_is_placed_on_the_line_line_of_gives() {
        let dir = scratch("lines");
        let root = dir.join("root");
        let sources = [
            (
                "src/b.ts",
                (0..1000).map(|i| "x".repeat(i % 7) + "\n").collect(),
            ),
            ("src/n.ts", "\n".repeat(2048)),
            ("src/e.ts", String::new()),
        ];
        let mut files = Files::new(&root);
        for (path, text) in &sources {
            fs::write(root.join(path), text).unwrap();
            for offset in -2..text.len() as i64 + 3 {
                let line = line_of(text.as_bytes(), offset);
                assert_eq!(files.at(path, offset), format!("{path}:{line}"), "{offset}");
            }
        }
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
