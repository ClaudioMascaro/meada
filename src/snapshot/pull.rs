//! `meada snapshot pull --workspace <manifest>` (ADR 0012): the store's current generation
//! fetched into the section's `local_dir`, checked and swapped in by rename. Everything read
//! from the store is hostile input: the ids, the generation document, every archive entry and
//! the mirrors' git configuration. A failure at any step leaves the pointer file and every held
//! generation as they were, and removes the `.partial` directory.
//!
//! Layout under `local_dir`: `current` (the node pointer file, one line `<generation id>
//! <fence>`), `pull.lock` (one pull at a time), and per held generation `<generation id>/`, with
//! `generation.json` (its document as stored) and per repository `<bundle id>/index.db` and
//! `<bundle id>/mirror/` (the checkout, `.git` included). A pull in progress writes only under
//! `<generation id>.partial/` and `current.new`.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

use super::store::{Store, Stored};
use super::{CURRENT_MAX, Config, DOCUMENT_MAX, Document, Entry, bundle_of, generation_id, is_id};
use crate::msg::Msg;
use crate::workspace::{Manifest, identity_problem};
use crate::{Result, repo};

/// The node pointer file under `local_dir`.
pub const POINTER: &str = "current";
/// The document of a generation, in its directory.
pub const DOCUMENT: &str = "generation.json";
/// The lock one pull at a time holds, under `local_dir`.
const LOCK: &str = "pull.lock";
/// The most the pointer file may hold: an id, a space, a `u64` fence and a newline.
const POINTER_MAX: u64 = 64;
// ponytail: the most bytes a generation's bundles may hold together, as its document records
// them (checked before any download), and the most its regular files may hold once extracted.
const GENERATION_BYTES_MAX: u64 = 256 << 30;
// ponytail: the most archive entries (files, directories and links) a generation's bundles may
// hold together.
const GENERATION_ENTRIES_MAX: u64 = 4_000_000;
// ponytail: the most a GNU long name or link, or a pax header, may hold in a bundle: the tar
// crate reads each into memory whole, so a scan refuses a larger one before extraction.
const HEADER_EXTENSION_MAX: u64 = 64 << 10;

/// What a pull did.
#[derive(Debug, PartialEq)]
pub enum Pulled {
    /// The pointer file now names this generation.
    Swapped(String),
    /// The node already held the store's current generation, and its pointer named it.
    Current(String),
    /// Another pull holds the lock of this `local_dir`.
    Running(PathBuf),
}

/// The generation id and fence `<local_dir>/current` names, `None` when there is none. The id
/// is checked against the id shape before it becomes part of a path.
pub fn current(local_dir: &Path) -> std::result::Result<Option<(String, u64)>, String> {
    let path = local_dir.join(POINTER);
    if fs::symlink_metadata(&path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
        return Ok(None);
    }
    let shown = path.display().to_string();
    let bad = || Msg::SnapshotPointerBad(&shown).text();
    let bytes = crate::config::read_bounded(&path, POINTER_MAX).map_err(|_| bad())?;
    let text = String::from_utf8(bytes).map_err(|_| bad())?;
    let (id, fence) = (text.strip_suffix('\n'))
        .and_then(|line| line.split_once(' '))
        .ok_or_else(bad)?;
    let digits = !fence.is_empty() && fence.bytes().all(|b| b.is_ascii_digit());
    match (is_id(id), digits.then(|| fence.parse().ok()).flatten()) {
        (true, Some(fence)) => Ok(Some((id.to_string(), fence))),
        _ => Err(bad()),
    }
}

/// `meada snapshot pull --workspace <manifest>`, once.
pub fn pull(manifest: &Path) -> Result<Pulled> {
    let listed = Manifest::read(manifest)?;
    let shown = manifest.display().to_string();
    let Some(config) = listed.snapshot else {
        return Err(Msg::SnapshotNoSection(&shown).text().into());
    };
    let Some(local) = config.local_dir.clone() else {
        return Err(Msg::SnapshotNoLocalDir(&shown).text().into());
    };
    let at = io_at(&local);
    fs::create_dir_all(&local).map_err(&at)?;
    let lock = (File::options().read(true).write(true).create(true))
        .truncate(false)
        .open(local.join(LOCK))
        .map_err(&at)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Ok(Pulled::Running(local)),
        Err(fs::TryLockError::Error(e)) => return Err(at(e).into()),
    }
    repo::isolate(&local);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let pulled = runtime.block_on(pull_locked(&config, &local));
    runtime.shutdown_background();
    Ok(pulled?)
}

/// The pull under the lock.
async fn pull_locked(config: &Config, local: &Path) -> std::result::Result<Pulled, String> {
    let previous = current(local)?.map(|(id, _)| id);
    let store = Store::connect(config)?;
    let key = config.key("current");
    let bytes = store.get(&key, CURRENT_MAX).await?;
    let id = store_pointer(
        &key,
        &bytes.ok_or_else(|| Msg::SnapshotNoCurrent(&key).text())?,
    )?;
    let dir = local.join(&id);
    let held = fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir());
    if held && previous.as_deref() == Some(id.as_str()) {
        return Ok(Pulled::Current(id));
    }
    // What a pull that crashed left.
    clear(local, |name| {
        name.strip_suffix(".partial").is_some_and(is_id)
    });
    // A generation held is one a pull verified: only the pointer moves back to it.
    if !held {
        let partial = local.join(format!("{id}.partial"));
        let fetched = match fetch(&store, config, &id, &partial).await {
            Ok(()) => fs::rename(&partial, &dir).map_err(io_at(&dir)),
            Err(e) => Err(e),
        };
        if let Err(e) = fetched {
            let _ = fs::remove_dir_all(&partial);
            return Err(e);
        }
    }
    swap(local, &id)?;
    clear(local, |name| {
        is_id(name) && name != id && Some(name) != previous.as_deref()
    });
    Ok(Pulled::Swapped(id))
}

/// The id the store's `current` holds: the id and a newline, nothing else.
fn store_pointer(key: &str, bytes: &[u8]) -> std::result::Result<String, String> {
    (std::str::from_utf8(bytes).ok())
        .and_then(|text| text.strip_suffix('\n'))
        .filter(|id| is_id(id))
        .map(str::to_string)
        .ok_or_else(|| Msg::SnapshotBadId(key).text())
}

/// Removes every directory directly under `local` whose name `doomed` picks; a failure is left
/// for the next pull.
fn clear(local: &Path, doomed: impl Fn(&str) -> bool) {
    for entry in fs::read_dir(local).into_iter().flatten().flatten() {
        let dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if dir && entry.file_name().to_str().is_some_and(&doomed) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// `current.new` written with `<id> 0` and renamed over the pointer file: fence `0`, since the
/// store's pointer carries none.
fn swap(local: &Path, id: &str) -> std::result::Result<(), String> {
    let new = local.join("current.new");
    let at = io_at(&new);
    let mut file = File::create(&new).map_err(&at)?;
    (file.write_all(format!("{id} 0\n").as_bytes()))
        .and_then(|()| file.sync_all())
        .map_err(&at)?;
    fs::rename(&new, local.join(POINTER)).map_err(at)
}

fn io_at(path: &Path) -> impl Fn(io::Error) -> String + use<> {
    let shown = path.display().to_string();
    move |e| format!("{shown}: {e}")
}

/// The generation `id` fetched into `partial`: its document checked, then per repository the
/// bundle downloaded and checked against the document's size and hash, extracted, its git
/// configuration replaced and the mirror checked against the document.
async fn fetch(
    store: &Store,
    config: &Config,
    id: &str,
    partial: &Path,
) -> std::result::Result<(), String> {
    let _ = fs::remove_dir_all(partial);
    fs::create_dir(partial).map_err(io_at(partial))?;
    let key = config.key(&format!("generations/{id}.json"));
    let bytes = store.get(&key, DOCUMENT_MAX).await?;
    let bytes = bytes.ok_or_else(|| Msg::SnapshotObjectBad(&key).text())?;
    let document = checked(config, id, &key, &bytes)?;
    let mut budget = Budget {
        bytes: GENERATION_BYTES_MAX,
        entries: GENERATION_ENTRIES_MAX,
    };
    for entry in &document.repositories {
        let bundle = bundle_of(&entry.bundle);
        let tar = partial.join(format!("{bundle}.tar"));
        let at = io_at(&tar);
        let mut out = BufWriter::new(File::create_new(&tar).map_err(&at)?);
        let expected = Stored {
            size: entry.size,
            blake3: entry.blake3.clone(),
        };
        store.download(&entry.bundle, &expected, &mut out).await?;
        out.into_inner().map_err(|e| at(e.into_error()))?;
        let dest = partial.join(bundle);
        let extracted = extract(&tar, &dest, &mut budget);
        extracted.map_err(|why| format!("{}: {why}", entry.bundle))?;
        fs::remove_file(&tar).map_err(&at)?;
        settle(&dest.join("mirror"), entry).map_err(|why| format!("{}: {why}", entry.identity))?;
    }
    fs::write(partial.join(DOCUMENT), &bytes).map_err(io_at(partial))
}

/// The document stored at `key` checked as the generation `id`, before any path or key is
/// built from it: every identity passes the manifest's rules (`identity_problem`, none listed
/// twice) and holds nothing a git remote URL cannot; every bundle key is `repos/<identity>/<bundle
/// id>.tar` under the prefix, no bundle id listed twice; every hash has its shape; the sizes sum
/// to at most `GENERATION_BYTES_MAX`; and the generation id derives from them. Each problem
/// names the document.
fn checked(
    config: &Config,
    id: &str,
    key: &str,
    bytes: &[u8],
) -> std::result::Result<Document, String> {
    let not = || Msg::SnapshotNotDocument(key).text();
    let document: Document = serde_json::from_slice(bytes).map_err(|_| not())?;
    if document.generation != id {
        return Err(not());
    }
    let hex = |text: &str, lens: &[usize]| {
        lens.contains(&text.len()) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    let (mut identities, mut bundles, mut total) = (HashSet::new(), HashSet::new(), 0u64);
    for entry in &document.repositories {
        let identity = entry.identity.as_str();
        let unsafe_char = |c: char| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\\');
        let problem = identity_problem(identity)
            .or_else(|| {
                (identity.chars().any(unsafe_char)).then(|| Msg::SnapshotIdentityChars.text())
            })
            .or_else(|| (!identities.insert(identity)).then(|| Msg::ListedTwice.text()));
        if let Some(problem) = problem {
            return Err(format!("{key}: {}: {problem}", identity.escape_debug()));
        }
        let prefix = config.key(&format!("repos/{identity}/"));
        let bundle = (entry.bundle.strip_prefix(&prefix))
            .and_then(|name| name.strip_suffix(".tar"))
            .filter(|bundle| is_id(bundle));
        let Some(bundle) = bundle else {
            let shown = entry.bundle.escape_debug().to_string();
            return Err(format!(
                "{key}: {}",
                Msg::SnapshotBadBundleKey(&shown).text()
            ));
        };
        if !bundles.insert(bundle) {
            return Err(format!("{key}: {bundle}: {}", Msg::ListedTwice.text()));
        }
        if !hex(&entry.blake3, &[64]) || !hex(&entry.head, &[40, 64]) {
            return Err(not());
        }
        total = (total.checked_add(entry.size))
            .filter(|total| *total <= GENERATION_BYTES_MAX)
            .ok_or_else(|| {
                format!(
                    "{key}: {}",
                    Msg::SnapshotBytesCeiling(GENERATION_BYTES_MAX).text()
                )
            })?;
    }
    let ids: Vec<(&str, &str)> = (document.repositories.iter())
        .map(|e| (e.identity.as_str(), bundle_of(&e.bundle)))
        .collect();
    if generation_id(&ids) != id {
        return Err(not());
    }
    Ok(document)
}

/// The extracted mirror made safe for `git`, then checked against the document's entry: `.git`
/// must be a directory; its `config` is replaced by a minimal one (`[core]` and the `origin`
/// whose URL gives the entry's identity), and its `hooks`, `commondir` and
/// `objects/info/alternates` (which point git elsewhere) are removed. Then, with `git` isolated
/// from any system or global configuration (`repo::isolate`), the mirror must be a checkout of
/// that identity at the entry's `HEAD`, with nothing changed, untracked or ignored, read
/// without descending into a submodule (`extract` already refused any nested repository).
fn settle(mirror: &Path, entry: &Entry) -> std::result::Result<(), String> {
    let git = mirror.join(".git");
    if !fs::symlink_metadata(&git).is_ok_and(|m| m.is_dir()) {
        return Err(Msg::SnapshotNoGit.text());
    }
    for doomed in ["config", "hooks", "commondir", "objects/info/alternates"] {
        let doomed = git.join(doomed);
        remove(&doomed).map_err(io_at(&doomed))?;
    }
    let config = format!(
        "[core]\n\trepositoryformatversion = 0\n\tbare = false\n\
         [remote \"origin\"]\n\turl = https://{}\n",
        entry.identity
    );
    let file = git.join("config");
    (File::create_new(&file))
        .and_then(|mut f| f.write_all(config.as_bytes()))
        .map_err(io_at(&file))?;
    let shown = mirror.display().to_string();
    if !repo::is_checkout(mirror) {
        return Err(Msg::NotACheckout(&shown).text());
    }
    let found = repo::identity(mirror);
    if found != entry.identity {
        let repo = entry.identity.as_str();
        return Err(Msg::IdentityMismatch {
            found: &found,
            repo,
        }
        .text());
    }
    let head = repo::head(mirror).map_err(|e| e.to_string())?;
    if head != entry.head {
        let expected = entry.head.as_str();
        return Err(Msg::SnapshotHead {
            found: &head,
            head: expected,
        }
        .text());
    }
    if !repo::unclean(mirror, false)
        .map_err(|e| e.to_string())?
        .is_empty()
    {
        let repo = entry.identity.as_str();
        return Err(Msg::SnapshotDirty {
            repo,
            mirror: &shown,
        }
        .text());
    }
    Ok(())
}

/// Removes `path` without following a symbolic link: a directory with all it holds, anything
/// else as a file. Nothing there is no error.
fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// What is left of a generation's ceilings as its bundles are extracted.
struct Budget {
    bytes: u64,
    entries: u64,
}

/// The bundle archive `tar` extracted into `dest` (created here) as hostile input, each entry
/// checked before it is written: `index.db` first, a regular file, then only `mirror` and what
/// is under it; no absolute path and no `.` or `..` component; nothing written through a
/// symbolic link or over an earlier entry; a symbolic link only to a relative target inside
/// the mirror, through no other symbolic link; a hard link only to a regular file of the
/// mirror extracted before it; no nested repository (a `.git` component below the mirror's
/// own, or anything under its `.git/modules`); no device, FIFO or other special file. Entries
/// and the bytes of
/// regular files are charged to `budget`. A regular file is written `0644`, or `0755` when the
/// archive gives it any execute bit.
fn extract(tar: &Path, dest: &Path, budget: &mut Budget) -> std::result::Result<(), String> {
    let mut file = File::open(tar).map_err(io_at(tar))?;
    if !extensions_bounded(&mut file).map_err(io_at(tar))? {
        return Err(Msg::SnapshotEntryHeader(HEADER_EXTENSION_MAX).text());
    }
    file.rewind().map_err(io_at(tar))?;
    fs::create_dir(dest).map_err(io_at(dest))?;
    let mut archive = tar::Archive::new(BufReader::new(file));
    let mirror = dest.join("mirror");
    // Each symbolic link: its name, its directory in the mirror and its target.
    let mut links: Vec<(String, String, String)> = Vec::new();
    let mut first = true;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let name = String::from_utf8_lossy(&entry.path_bytes())
            .escape_debug()
            .to_string();
        let refuse = |why: Msg| format!("entry `{name}`: {}", why.text());
        let fail = |e: io::Error| match e.kind() {
            io::ErrorKind::AlreadyExists => refuse(Msg::SnapshotEntryThrough),
            _ => format!("entry `{name}`: {e}"),
        };
        let ceiling = Msg::SnapshotEntriesCeiling(GENERATION_ENTRIES_MAX);
        budget.entries = budget
            .entries
            .checked_sub(1)
            .ok_or_else(|| refuse(ceiling))?;
        let path = entry.path().map_err(|_| refuse(Msg::SnapshotEntryPath))?;
        let path = path.into_owned();
        let parts = normal(&path).ok_or_else(|| refuse(Msg::SnapshotEntryPath))?;
        let kind = entry.header().entry_type();
        let placed = match first {
            true => parts == [OsStr::new("index.db")] && kind == tar::EntryType::Regular,
            false => parts[0] == "mirror" && (parts.len() > 1 || kind == tar::EntryType::Directory),
        };
        if !placed {
            return Err(refuse(Msg::SnapshotEntryPath));
        }
        // Only the mirror's own `.git`: git runs the configuration of a nested repository when
        // `git status` descends into it as a submodule.
        let git = |part: &OsStr| part.eq_ignore_ascii_case(".git");
        let nested = parts.iter().skip(2).any(|part| git(part))
            || (parts.len() > 2 && git(parts[1]) && parts[2] == "modules");
        if nested {
            return Err(refuse(Msg::SnapshotEntryNested));
        }
        first = false;
        let target = dest.join(&path);
        match kind {
            tar::EntryType::Directory => {
                directories(dest, &parts, true).map_err(fail)?;
                match fs::symlink_metadata(&target) {
                    Ok(meta) if meta.is_dir() => {}
                    Ok(_) => return Err(refuse(Msg::SnapshotEntryThrough)),
                    Err(_) => fs::create_dir(&target).map_err(fail)?,
                }
            }
            tar::EntryType::Regular => {
                let size = entry.size();
                let ceiling = Msg::SnapshotBytesCeiling(GENERATION_BYTES_MAX);
                budget.bytes = budget
                    .bytes
                    .checked_sub(size)
                    .ok_or_else(|| refuse(ceiling))?;
                directories(dest, &parts, true).map_err(fail)?;
                let mode = entry.header().mode().unwrap_or(0o644);
                let mut out = create(&target, mode).map_err(fail)?;
                let copied = io::copy(&mut entry, &mut out).map_err(fail)?;
                if copied != size {
                    return Err(fail(io::ErrorKind::UnexpectedEof.into()));
                }
            }
            tar::EntryType::Symlink => {
                let utf8 = |bytes: &[u8]| std::str::from_utf8(bytes).ok().map(str::to_string);
                let to = entry.link_name_bytes().and_then(|bytes| utf8(&bytes));
                let at: Option<Vec<&str>> = parts[1..parts.len() - 1]
                    .iter()
                    .map(|p| p.to_str())
                    .collect();
                let (Some(to), Some(at)) = (to, at) else {
                    return Err(refuse(Msg::SnapshotEntryLink));
                };
                let at = at.join("/");
                if !inside(&at, &to) {
                    return Err(refuse(Msg::SnapshotEntryLink));
                }
                directories(dest, &parts, true).map_err(fail)?;
                symlink(&to, &target).map_err(fail)?;
                links.push((name.clone(), at, to));
            }
            tar::EntryType::Link => {
                let to = entry.link_name().ok().flatten();
                let to = to.as_deref().and_then(normal);
                let Some(to) = to.filter(|to| to.len() > 1 && to[0] == "mirror") else {
                    return Err(refuse(Msg::SnapshotEntryLink));
                };
                let file = directories(dest, &to, false)
                    .and_then(|()| fs::symlink_metadata(dest.join(to.iter().collect::<PathBuf>())));
                let source = dest.join(to.iter().collect::<PathBuf>());
                if !file.is_ok_and(|meta| meta.is_file()) {
                    return Err(refuse(Msg::SnapshotEntryLink));
                }
                directories(dest, &parts, true).map_err(fail)?;
                fs::hard_link(&source, &target).map_err(fail)?;
            }
            _ => return Err(refuse(Msg::SnapshotEntrySpecial)),
        }
    }
    if first {
        return Err(Msg::SnapshotEntryPath.text());
    }
    // Once every entry is there: no link's target goes through another link, so the target
    // resolves where its lexical fold says, inside the mirror.
    for (name, at, to) in &links {
        let steps: Vec<&str> = to.split('/').collect();
        for end in 1..steps.len() {
            let folded = repo::relative(at, &steps[..end].join("/")).unwrap_or_default();
            let link = fs::symlink_metadata(mirror.join(folded));
            if link.is_ok_and(|meta| meta.file_type().is_symlink()) {
                return Err(format!(
                    "entry `{name}`: {}",
                    Msg::SnapshotEntryThrough.text()
                ));
            }
        }
    }
    Ok(())
}

/// The components of `path` when all are names (no root, `.` or `..`), at least one.
fn normal(path: &Path) -> Option<Vec<&OsStr>> {
    let parts: Option<Vec<&OsStr>> = (path.components())
        .map(|c| match c {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    parts.filter(|parts| !parts.is_empty())
}

/// Whether the symbolic link target `to`, relative and folded from the mirror directory `at`,
/// stays inside the mirror.
fn inside(at: &str, to: &str) -> bool {
    repo::relative(at, to).is_some_and(|p| p != ".." && !p.starts_with("../"))
}

/// The directories above the entry `parts` under `dest`, each a directory and no symbolic link;
/// with `make`, a missing one is created. Any other file in the way is `AlreadyExists`.
fn directories(dest: &Path, parts: &[&OsStr], make: bool) -> io::Result<()> {
    let mut at = dest.to_path_buf();
    for part in &parts[..parts.len() - 1] {
        at.push(part);
        match fs::symlink_metadata(&at) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(io::ErrorKind::AlreadyExists.into()),
            Err(e) if make && e.kind() == io::ErrorKind::NotFound => fs::create_dir(&at)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// A new regular file at `path`, never through a link there: `0755` when `mode` has an execute
/// bit, `0644` otherwise.
fn create(path: &Path, mode: u32) -> io::Result<File> {
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if mode & 0o111 != 0 { 0o755 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path)
}

#[cfg(unix)]
fn symlink(to: &str, path: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(to, path)
}

/// Only Unix nodes hold mirrors with symbolic links.
#[cfg(not(unix))]
fn symlink(_: &str, _: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Whether every GNU long name, long link and pax header of the archive `file` holds at most
/// `HEADER_EXTENSION_MAX` bytes. Reads only the headers, seeking past each entry's data.
fn extensions_bounded(file: &mut File) -> io::Result<bool> {
    let mut block = [0u8; 512];
    loop {
        // A short or empty end is the tar crate's to report.
        if file.read_exact(&mut block).is_err() || block.iter().all(|b| *b == 0) {
            return Ok(true);
        }
        let header = tar::Header::from_byte_slice(&block);
        let size = header.entry_size()?;
        let kind = header.entry_type();
        let extension = kind.is_gnu_longname()
            || kind.is_gnu_longlink()
            || kind.is_pax_local_extensions()
            || kind.is_pax_global_extensions();
        if extension && size > HEADER_EXTENSION_MAX {
            return Ok(false);
        }
        let skip = (size.div_ceil(512).checked_mul(512))
            .and_then(|n| i64::try_from(n).ok())
            .ok_or(io::ErrorKind::InvalidData)?;
        file.seek(io::SeekFrom::Current(skip))?;
    }
}

#[cfg(test)]
mod tests {
    use super::super::Credentials;
    use super::*;
    use tar::EntryType;

    fn config() -> Config {
        Config {
            endpoint: "https://s3.example.internal".into(),
            region: "us-east-1".into(),
            bucket: "meada-store".into(),
            prefix: "prod".into(),
            path_style: false,
            credentials: Credentials::Instance,
            local_dir: None,
        }
    }

    fn scratch(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meada-pull-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Ids.

    #[test]
    fn the_store_pointer_is_an_id_and_a_newline() {
        let key = "prod/current";
        assert_eq!(
            store_pointer(key, b"0123456789abcdef\n").unwrap(),
            "0123456789abcdef"
        );
        let refused = |bytes: &[u8]| {
            let e = store_pointer(key, bytes).unwrap_err();
            assert!(e.starts_with("prod/current: "), "{e}");
        };
        refused(b"0123456789abcdef");
        refused(b"0123456789abcdef\n\n");
        refused(b"0123456789abcdef 0\n");
        refused(&[0xff; 17]);
    }

    #[test]
    fn a_store_pointer_with_path_separators_is_refused() {
        assert!(store_pointer("k", b"../../../etc/pw\n").is_err());
        assert!(store_pointer("k", b"0123456/89abcdef\n").is_err());
        assert!(store_pointer("k", b"0123456\\89abcdef\n").is_err());
    }

    #[test]
    fn a_store_pointer_of_dot_dot_is_refused() {
        assert!(store_pointer("k", b"..\n").is_err());
        assert!(store_pointer("k", b"..............\n").is_err());
    }

    #[test]
    fn an_uppercase_store_pointer_is_refused() {
        assert!(store_pointer("k", b"0123456789ABCDEF\n").is_err());
    }

    #[test]
    fn a_store_pointer_of_the_wrong_length_is_refused() {
        assert!(store_pointer("k", b"0123456789abcde\n").is_err());
        assert!(store_pointer("k", b"0123456789abcdef0\n").is_err());
    }

    #[test]
    fn an_empty_store_pointer_is_refused() {
        assert!(store_pointer("k", b"").is_err());
        assert!(store_pointer("k", b"\n").is_err());
    }

    #[test]
    fn the_node_pointer_file_is_an_id_and_a_fence() {
        let dir = scratch("pointer");
        assert_eq!(current(&dir).unwrap(), None);
        let file = dir.join(POINTER);
        fs::write(&file, "0123456789abcdef 7\n").unwrap();
        assert_eq!(current(&dir).unwrap(), Some(("0123456789abcdef".into(), 7)));
        for bad in [
            "0123456789abcdef\n",
            "0123456789abcdef 7",
            "0123456789abcdef -1\n",
            "0123456789abcdef \n",
            "../../../etc/pw 0\n",
            "0123456789ABCDEF 0\n",
        ] {
            fs::write(&file, bad).unwrap();
            assert!(current(&dir).unwrap_err().contains("delete it"), "{bad}");
        }
        fs::write(&file, "0".repeat(POINTER_MAX as usize + 1)).unwrap();
        assert!(current(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    // The generation document.

    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

    /// A document of the generation of `entries` (identity, bundle key), with its id.
    fn document(entries: &[(&str, &str)]) -> (String, Vec<u8>) {
        let bundles: Vec<(&str, &str)> = (entries.iter())
            .map(|(identity, key)| (*identity, bundle_of(key)))
            .collect();
        let id = generation_id(&bundles);
        let repositories: Vec<serde_json::Value> = (entries.iter())
            .map(|(identity, key)| {
                serde_json::json!({"identity": identity, "head": HEAD, "bundle": key,
                    "blocks": {}, "size": 10, "blake3": "a".repeat(64)})
            })
            .collect();
        let text = serde_json::json!({"generation": id, "repositories": repositories});
        (id, serde_json::to_vec(&text).unwrap())
    }

    fn check(entries: &[(&str, &str)]) -> std::result::Result<Document, String> {
        let (id, bytes) = document(entries);
        checked(&config(), &id, "prod/generations/g.json", &bytes)
    }

    const WEB: (&str, &str) = (
        "example.com/acme/web",
        "prod/repos/example.com/acme/web/1111111111111111.tar",
    );

    fn refused_naming_the_document(entries: &[(&str, &str)]) -> String {
        let e = check(entries).map(drop).unwrap_err();
        assert!(e.starts_with("prod/generations/g.json: "), "{e}");
        e
    }

    #[test]
    fn a_document_of_its_generation_passes() {
        let api = (
            "example.com/acme/api",
            "prod/repos/example.com/acme/api/2222222222222222.tar",
        );
        assert_eq!(check(&[WEB, api]).unwrap().repositories.len(), 2);
    }

    #[test]
    fn a_document_of_another_generation_is_refused() {
        let (_, bytes) = document(&[WEB]);
        let e = checked(&config(), "0000000000000000", "d", &bytes).map(drop);
        assert!(e.unwrap_err().starts_with("d: "));
        assert!(checked(&config(), "0000000000000000", "d", b"{").is_err());
    }

    #[test]
    fn an_identity_with_a_dot_dot_segment_is_refused() {
        let e = refused_naming_the_document(&[(
            "example.com/../web",
            "prod/repos/example.com/../web/1111111111111111.tar",
        )]);
        assert!(e.contains("not a repository identity"), "{e}");
    }

    #[test]
    fn an_identity_with_an_empty_segment_is_refused() {
        for identity in ["example.com//web", "/etc", "example.com/web/", ""] {
            let key = format!("prod/repos/{identity}/1111111111111111.tar");
            let e = refused_naming_the_document(&[(identity, &key)]);
            assert!(e.contains("not a repository identity"), "{e}");
        }
    }

    #[test]
    fn a_local_identity_is_refused() {
        let e = refused_naming_the_document(&[(
            "local/web",
            "prod/repos/local/web/1111111111111111.tar",
        )]);
        assert!(e.contains("local identity"), "{e}");
    }

    #[test]
    fn an_identity_listed_twice_is_refused() {
        let again = (
            WEB.0,
            "prod/repos/example.com/acme/web/2222222222222222.tar",
        );
        let e = refused_naming_the_document(&[WEB, again]);
        assert!(e.contains("listed twice"), "{e}");
    }

    #[test]
    fn an_identity_a_git_config_cannot_hold_is_refused() {
        for identity in [
            "example.com/a\"b",
            "example.com/a\nb",
            "example.com/a b",
            "x/a\\b",
        ] {
            let key = format!("prod/repos/{identity}/1111111111111111.tar");
            let e = refused_naming_the_document(&[(identity, &key)]);
            assert!(e.contains("control character") && !e.contains('\n'), "{e}");
        }
    }

    #[test]
    fn a_bundle_key_that_is_not_the_identity_s_with_an_id_is_refused() {
        for key in [
            "prod/repos/example.com/acme/web/111111111111111.tar",
            "prod/repos/example.com/acme/web/1111111111111111A.tar",
            "prod/repos/example.com/acme/web/AAAAAAAAAAAAAAAA.tar",
            "prod/repos/example.com/acme/web/.tar",
            "prod/repos/example.com/acme/web/../../../x/1111111111111111.tar",
            "prod/repos/example.com/acme/web/11111111/11111111.tar",
            "prod/repos/example.com/acme/web/1111111111111111",
            "prod/repos/example.com/acme/api/1111111111111111.tar",
            "other/repos/example.com/acme/web/1111111111111111.tar",
            "",
        ] {
            let e = refused_naming_the_document(&[(WEB.0, key)]);
            assert!(e.contains("bundle key"), "{e}");
        }
    }

    #[test]
    fn a_bundle_id_listed_twice_is_refused() {
        let api = (
            "example.com/acme/api",
            "prod/repos/example.com/acme/api/1111111111111111.tar",
        );
        let e = refused_naming_the_document(&[WEB, api]);
        assert!(e.contains("listed twice"), "{e}");
    }

    #[test]
    fn a_document_past_the_byte_ceiling_is_refused_before_any_download() {
        let (id, bytes) = document(&[WEB]);
        let mut text: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        text["repositories"][0]["size"] = (GENERATION_BYTES_MAX + 1).into();
        let bytes = serde_json::to_vec(&text).unwrap();
        let e = checked(&config(), &id, "d", &bytes).map(drop).unwrap_err();
        assert!(e.contains("ceiling"), "{e}");
        text["repositories"][0]["size"] = GENERATION_BYTES_MAX.into();
        let bytes = serde_json::to_vec(&text).unwrap();
        assert!(checked(&config(), &id, "d", &bytes).is_ok());
    }

    // Extraction.

    /// An entry as a tar header writes it, its path and link name raw: no check of the tar
    /// crate's own stands between the test and the reader.
    #[derive(Clone, Copy)]
    enum Item<'a> {
        File(&'a [u8], &'a [u8]),
        Dir(&'a [u8]),
        Link(&'a [u8], EntryType, &'a [u8]),
    }

    fn archive(items: &[Item]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for item in items {
            let (path, kind, data, link): (&[u8], _, &[u8], &[u8]) = match item {
                Item::File(path, data) => (path, EntryType::Regular, data, b""),
                Item::Dir(path) => (path, EntryType::Directory, b"", b""),
                Item::Link(path, kind, to) => (path, *kind, b"", to),
            };
            let mut header = tar::Header::new_gnu();
            header.as_old_mut().name[..path.len()].copy_from_slice(path);
            header.as_old_mut().linkname[..link.len()].copy_from_slice(link);
            header.set_entry_type(kind);
            header.set_size(data.len() as u64);
            header.set_mode(if path.ends_with(b".sh") {
                0o4755
            } else {
                0o600
            });
            header.set_cksum();
            builder.append(&header, data).unwrap();
        }
        builder.into_inner().unwrap()
    }

    fn budget() -> Budget {
        Budget {
            bytes: GENERATION_BYTES_MAX,
            entries: GENERATION_ENTRIES_MAX,
        }
    }

    /// `bytes` extracted into `<scratch>/b`, with `budget`.
    fn extracted(
        case: &str,
        bytes: &[u8],
        budget: &mut Budget,
    ) -> (PathBuf, std::result::Result<(), String>) {
        let dir = scratch(case);
        let tar = dir.join("b.tar");
        fs::write(&tar, bytes).unwrap();
        let dest = dir.join("b");
        let result = extract(&tar, &dest, budget);
        (dest, result)
    }

    fn with_mirror(rest: &[Item]) -> Vec<u8> {
        let mut items = vec![Item::File(b"index.db", b"sqlite"), Item::Dir(b"mirror")];
        items.extend_from_slice(rest);
        archive(&items)
    }

    fn refused(case: &str, rest: &[Item]) -> String {
        let (dest, result) = extracted(case, &with_mirror(rest), &mut budget());
        let _ = fs::remove_dir_all(dest.parent().unwrap());
        result.unwrap_err()
    }

    #[cfg(unix)]
    #[test]
    fn a_bundle_extracts_its_files_directories_and_links() {
        use std::os::unix::fs::PermissionsExt;
        let bytes = with_mirror(&[
            Item::File(b"mirror/run.sh", b"echo"),
            Item::Dir(b"mirror/src"),
            Item::File(b"mirror/src/a.ts", b"export {}"),
            Item::Link(b"mirror/src/l", EntryType::Symlink, b"../run.sh"),
            Item::Link(b"mirror/h", EntryType::Link, b"mirror/src/a.ts"),
            Item::File(b"mirror/deep/er/x", b"x"),
            Item::Dir(b"mirror/.git"),
            Item::File(b"mirror/.git/config", b"[core]"),
            Item::File(b"mirror/src/.gitignore", b"x"),
        ]);
        let (dest, result) = extracted("valid", &bytes, &mut budget());
        result.unwrap();
        assert_eq!(fs::read(dest.join("index.db")).unwrap(), b"sqlite");
        assert_eq!(fs::read(dest.join("mirror/src/l")).unwrap(), b"echo");
        assert_eq!(
            fs::read_link(dest.join("mirror/src/l")).unwrap(),
            Path::new("../run.sh")
        );
        assert_eq!(fs::read(dest.join("mirror/h")).unwrap(), b"export {}");
        assert_eq!(fs::read(dest.join("mirror/deep/er/x")).unwrap(), b"x");
        let mode = |p: &str| fs::metadata(dest.join(p)).unwrap().permissions().mode() & 0o7777;
        // No setuid bit, the execute bit kept, and the owner may read what it extracted.
        let (run, a) = (mode("mirror/run.sh"), mode("mirror/src/a.ts"));
        assert_eq!((run & 0o7000, run & 0o700), (0, 0o700), "{run:o}");
        assert_eq!((a & 0o111, a & 0o600), (0, 0o600), "{a:o}");
        let _ = fs::remove_dir_all(dest.parent().unwrap());
    }

    #[test]
    fn an_absolute_path_is_refused() {
        let e = refused("absolute", &[Item::File(b"/tmp/meada-pull-absolute", b"x")]);
        assert!(e.contains("absolute path"), "{e}");
        assert!(!Path::new("/tmp/meada-pull-absolute").exists());
    }

    #[test]
    fn a_dot_dot_component_is_refused() {
        let e = refused("dotdot", &[Item::File(b"mirror/../../escaped", b"x")]);
        assert!(e.contains("`..`"), "{e}");
        let e = refused("dot", &[Item::File(b"./mirror/x", b"x")]);
        assert!(e.contains("`..`"), "{e}");
    }

    #[test]
    fn an_entry_outside_index_db_and_the_mirror_is_refused() {
        let e = refused("outside", &[Item::File(b"other/x", b"x")]);
        assert!(e.contains("index.db first"), "{e}");
        let (dest, result) = extracted("first", &archive(&[Item::Dir(b"mirror")]), &mut budget());
        assert!(result.unwrap_err().contains("index.db first"));
        let _ = fs::remove_dir_all(dest.parent().unwrap());
        let (dest, result) = extracted("empty", &archive(&[]), &mut budget());
        assert!(result.unwrap_err().contains("index.db first"));
        let _ = fs::remove_dir_all(dest.parent().unwrap());
    }

    #[test]
    fn a_symbolic_link_leaving_the_mirror_is_refused() {
        for to in [
            &b"../index.db"[..],
            b"../../x",
            b"/etc/passwd",
            b"src/../../x",
        ] {
            let e = refused(
                "symlink-out",
                &[Item::Link(b"mirror/l", EntryType::Symlink, to)],
            );
            assert!(e.contains("leaves the repository"), "{e}");
        }
    }

    #[test]
    fn a_hard_link_leaving_the_mirror_is_refused() {
        for to in [
            &b"index.db"[..],
            b"mirror/../index.db",
            b"/etc/passwd",
            b"mirror/none",
        ] {
            let e = refused(
                "hardlink-out",
                &[Item::Link(b"mirror/h", EntryType::Link, to)],
            );
            assert!(e.contains("leaves the repository"), "{e}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_entry_written_through_a_symbolic_link_is_refused() {
        let e = refused(
            "through",
            &[
                Item::Dir(b"mirror/sub"),
                Item::Link(b"mirror/l", EntryType::Symlink, b"sub"),
                Item::File(b"mirror/l/x", b"x"),
            ],
        );
        assert!(e.contains("through a symbolic link"), "{e}");
        // Nor over one, or over any earlier entry.
        for item in [
            Item::File(b"mirror/l", b"x"),
            Item::Dir(b"mirror/l"),
            Item::Link(b"mirror/l", EntryType::Symlink, b"sub"),
        ] {
            let e = refused(
                "over",
                &[
                    Item::Dir(b"mirror/sub"),
                    Item::Link(b"mirror/l", EntryType::Symlink, b"sub"),
                    item,
                ],
            );
            assert!(e.contains("through a symbolic link"), "{e}");
        }
    }

    /// Each link stays inside on its own, but one resolves through the other: `s` sits three
    /// levels down and points at the top, so `s/../..` climbs out of the mirror.
    #[cfg(unix)]
    #[test]
    fn a_link_through_another_link_is_refused() {
        let e = refused(
            "chain",
            &[
                Item::Link(b"mirror/a/b/c/l", EntryType::Symlink, b"s/../.."),
                Item::Link(b"mirror/a/b/c/s", EntryType::Symlink, b"../../../t"),
            ],
        );
        assert!(
            e.contains("mirror/a/b/c/l") && e.contains("through a symbolic link"),
            "{e}"
        );
    }

    /// A pulled mirror never needs a nested repository: git would run its configuration when
    /// `git status` descends into it as a submodule.
    #[test]
    fn a_nested_git_directory_file_or_link_is_refused() {
        for item in [
            Item::Dir(b"mirror/sub/.git"),
            Item::File(b"mirror/sub/.git/config", b"[core]"),
            Item::File(b"mirror/sub/.git", b"gitdir: ../.git/modules/sub"),
            Item::Link(b"mirror/sub/.git", EntryType::Symlink, b"../x"),
            Item::Link(b"mirror/a/b/.GIT", EntryType::Symlink, b"../x"),
            Item::File(b"mirror/a/.git/x/y", b"x"),
        ] {
            let e = refused("nested", &[item]);
            assert!(
                e.contains("entry `mirror/") && e.contains("nested repository"),
                "{e}"
            );
        }
    }

    #[test]
    fn the_modules_of_the_mirror_s_own_git_are_refused() {
        for item in [
            Item::Dir(b"mirror/.git/modules"),
            Item::File(b"mirror/.git/modules/sub/config", b"[core]"),
        ] {
            let e = refused("modules", &[Item::Dir(b"mirror/.git"), item]);
            assert!(
                e.contains("entry `mirror/.git/modules") && e.contains("nested repository"),
                "{e}"
            );
        }
    }

    #[test]
    fn devices_fifos_and_other_special_files_are_refused() {
        for kind in [
            EntryType::Char,
            EntryType::Block,
            EntryType::Fifo,
            EntryType::Continuous,
            EntryType::XGlobalHeader,
        ] {
            let e = refused("special", &[Item::Link(b"mirror/dev", kind, b"")]);
            assert!(e.contains("special file"), "{kind:?}: {e}");
        }
        // A sparse file the tar crate cannot read stops it first; one it can is special.
        let sparse = [Item::Link(b"mirror/dev", EntryType::GNUSparse, b"")];
        assert!(!refused("sparse", &sparse).is_empty());
    }

    #[test]
    fn extraction_past_the_entry_ceiling_is_refused() {
        let bytes = with_mirror(&[Item::File(b"mirror/a", b"a")]);
        let mut small = Budget {
            bytes: GENERATION_BYTES_MAX,
            entries: 2,
        };
        let (dest, result) = extracted("entries", &bytes, &mut small);
        let e = result.unwrap_err();
        assert!(
            e.contains(&format!(
                "ceiling of {GENERATION_ENTRIES_MAX} archive entries"
            )),
            "{e}"
        );
        let _ = fs::remove_dir_all(dest.parent().unwrap());
    }

    #[test]
    fn extraction_past_the_byte_ceiling_is_refused() {
        let bytes = with_mirror(&[Item::File(b"mirror/a", b"abcd")]);
        let mut small = Budget {
            bytes: 9,
            entries: GENERATION_ENTRIES_MAX,
        };
        let (dest, result) = extracted("bytes", &bytes, &mut small);
        let e = result.unwrap_err();
        assert!(
            e.contains("mirror/a") && e.contains("bytes per generation"),
            "{e}"
        );
        let _ = fs::remove_dir_all(dest.parent().unwrap());
    }

    #[test]
    fn a_long_name_past_the_header_ceiling_is_refused_before_extraction() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..13].copy_from_slice(b"././@LongLink");
        header.set_entry_type(EntryType::GNULongName);
        let long = vec![b'a'; HEADER_EXTENSION_MAX as usize + 1];
        header.set_size(long.len() as u64);
        header.set_cksum();
        builder.append(&header, long.as_slice()).unwrap();
        let (dest, result) = extracted("longname", &builder.into_inner().unwrap(), &mut budget());
        assert!(result.unwrap_err().contains("pax header"));
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(dest.parent().unwrap());
    }
}
