//! Repository root and identity, read through the `git` command line.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::Result;
use crate::msg::Msg;

/// The top level of the git repository containing `dir`, or `dir` itself outside git.
pub fn root(dir: &Path) -> Result<PathBuf> {
    match git(dir, &["rev-parse", "--show-toplevel"]) {
        Some(top) => Ok(PathBuf::from(top)),
        None => Ok(dir.canonicalize()?),
    }
}

/// Whether `dir` is the top level of a checkout with a working tree: not a bare clone, not a
/// directory inside a checkout, not a directory outside git.
pub fn is_checkout(dir: &Path) -> bool {
    let (Some(top), Ok(dir)) = (
        git(dir, &["rev-parse", "--show-toplevel"]),
        dir.canonicalize(),
    ) else {
        return false;
    };
    Path::new(&top).canonicalize().is_ok_and(|top| top == dir)
}

/// The normalized `origin` remote, or `local/<root directory name>` without one.
pub fn identity(root: &Path) -> String {
    match git(root, &["remote", "get-url", "origin"]) {
        Some(url) if !url.is_empty() => normalize_remote(&url),
        _ => {
            let name = root.file_name().unwrap_or_default().to_string_lossy();
            format!("local/{name}")
        }
    }
}

/// The commit `HEAD` names in the checkout at `root`.
pub fn head(root: &Path) -> Result<String> {
    Ok(run(root, &["rev-parse", "--verify", "HEAD"])?
        .trim()
        .to_string())
}

/// What `git status` lists in the checkout at `root`, changed, untracked and ignored files
/// alike: empty for a clean mirror. It writes nothing, not even the index's stat cache. With
/// `submodules` false (a mirror pulled from a store) it does not descend into a submodule,
/// whose configuration git would run.
pub fn unclean(root: &Path, submodules: bool) -> Result<String> {
    let mut args = vec!["--no-optional-locks", "status", "--porcelain", "--ignored"];
    if !submodules {
        args.push("--ignore-submodules=all");
    }
    run(root, &args)
}

/// Whether the checkout at `root` differs from `HEAD`: a changed, deleted or untracked file, but
/// not an ignored one (so never the index's own `.meada/`), unlike `unclean`. Untracked files
/// count whatever `status.showUntrackedFiles` says, since the index reads them. It writes
/// nothing, not even the index's stat cache.
pub fn changed(root: &Path) -> Result<bool> {
    let args = [
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--untracked-files=normal",
    ];
    let status = run(root, &args)?;
    Ok(!status.is_empty())
}

/// Lower-cased host and path, with no scheme, user, port or `.git` suffix, so the SSH and
/// HTTPS forms of one remote are equal.
pub fn normalize_remote(url: &str) -> String {
    let url = url.trim();
    let (host, path) = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').unwrap_or((rest, "")),
        // scp-like syntax: `user@host:path`
        None => url.split_once(':').unwrap_or(("", url)),
    };
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split_once(':').map_or(host, |(h, _)| h);
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    format!("{host}/{path}").trim_matches('/').to_lowercase()
}

/// `rel` joined to the directory `dir`, `.` and `..` folded without touching the file system: a
/// path of the repository, or one that starts with `..` segments when it leaves it. `None` when
/// `rel` is absolute.
pub fn relative(dir: &str, rel: &str) -> Option<String> {
    if rel.starts_with('/') || rel.contains('\\') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Whether `rev` names the commit `HEAD` names in the checkout at `root`.
fn resolves_to_head(root: &Path, rev: &str) -> bool {
    let commit = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    );
    commit.is_some_and(|commit| head(root).is_ok_and(|h| h == commit))
}

/// What a change did to a path.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// Inclusive 1-based line ranges of the file as it is now; a deletion touches the line
    /// before it.
    Lines(Vec<(usize, usize)>),
    /// All of the file: an untracked file, a path given as a whole, or a change with no line
    /// (a binary file, a mode change, a new empty file).
    Untracked,
    /// A path the change removes.
    Deleted,
}

/// Where a change list comes from.
#[derive(Clone, Copy, Debug)]
pub enum Changes<'a> {
    /// The working tree against `HEAD`, untracked files included.
    Uncommitted,
    /// The commits from `base` to `head`, which must be the checked-out commit of a clean tree:
    /// the index answers about the working tree.
    Range { base: &'a str, head: &'a str },
    /// Repository-relative paths, each changed as a whole; one not on disk is deleted.
    Files(&'a [String]),
}

impl Changes<'_> {
    /// The changed paths of the repository at `root`, each with what changed.
    pub fn read(&self, root: &Path) -> Result<Vec<(String, Change)>> {
        match *self {
            Changes::Uncommitted => working_tree(root, true),
            Changes::Range { base, head } => {
                // `git diff` would take a revision starting with `-` as an option.
                if let Some(rev) = [base, head].into_iter().find(|r| r.starts_with('-')) {
                    return Err(Msg::TraceChangesValue(rev).text().into());
                }
                if !resolves_to_head(root, head) {
                    return Err(Msg::RangeNotCheckedOut { head }.text().into());
                }
                if changed(root)? {
                    return Err(Msg::TreeNotClean.text().into());
                }
                diff(root, &[base, head], true)
            }
            Changes::Files(paths) => paths
                .iter()
                .map(|path| {
                    let Some(rel) = relative("", path)
                        .filter(|r| !r.is_empty() && r != ".." && !r.starts_with("../"))
                    else {
                        return Err(Msg::PathOutside { path }.text().into());
                    };
                    let full = root.join(&rel);
                    if full.is_dir() {
                        return Err(Msg::PathIsDirectory { path }.text().into());
                    }
                    let change = match full.exists() {
                        true => Change::Untracked,
                        false => Change::Deleted,
                    };
                    Ok((rel, change))
                })
                .collect(),
        }
    }
}

/// Every repository-relative path whose working-tree content differs from `HEAD`, tracked or
/// untracked (ignored files are left out, deleted files too, since nothing of theirs is left).
/// Before the first commit, every file counts as untracked.
pub fn uncommitted(root: &Path) -> Result<Vec<(String, Change)>> {
    working_tree(root, false)
}

/// The working tree against `HEAD`; with `whole`, as `diff` reads it.
fn working_tree(root: &Path, whole: bool) -> Result<Vec<(String, Change)>> {
    let untracked = |more: &[&str]| -> Result<Vec<(String, Change)>> {
        let args = [&["ls-files", "--others", "--exclude-standard", "-z"], more].concat();
        Ok(run(root, &args)?
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(|p| (p.to_string(), Change::Untracked))
            .collect())
    };
    if git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_none() {
        return untracked(&["--cached"]);
    }
    let mut changes = diff(root, &["HEAD"], whole)?;
    changes.extend(untracked(&[])?);
    Ok(changes)
}

/// The paths `git diff <revs>` lists, with the lines of the new side each one changed. With
/// `whole`, an entry with no line (a binary file, a mode change, a new empty file) is
/// `Untracked` and a removed path `Deleted`; without it both are left out.
fn diff(root: &Path, revs: &[&str], whole: bool) -> Result<Vec<(String, Change)>> {
    // Explicit prefixes and no external diff, whatever the user's git configuration says.
    let mut args = vec!["-c", "core.quotePath=false", "diff"];
    args.extend(revs);
    args.extend([
        "--no-renames",
        "--no-ext-diff",
        "--no-color",
        "--unified=0",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ]);
    Ok(parse_diff(&run(root, &args)?, whole))
}

/// One entry of a diff: its path, whether it removes the file, the new side's changed lines.
struct Entry {
    path: Option<String>,
    deleted: bool,
    lines: Vec<(usize, usize)>,
}

/// `diff`'s reading of git's output.
fn parse_diff(diff: &str, whole: bool) -> Vec<(String, Change)> {
    let mut changes = Vec::new();
    let mut finish = |entry: Option<Entry>| {
        let Some(Entry {
            path: Some(path),
            deleted,
            lines,
        }) = entry
        else {
            return;
        };
        let change = match (deleted, lines.is_empty()) {
            (true, _) => whole.then_some(Change::Deleted),
            (false, false) => Some(Change::Lines(lines)),
            (false, true) => whole.then_some(Change::Untracked),
        };
        changes.extend(change.map(|c| (path, c)));
    };
    let mut entry: Option<Entry> = None;
    let mut header = false;
    for line in diff.lines() {
        if let Some(paths) = line.strip_prefix("diff --git ") {
            finish(entry.take());
            entry = Some(Entry {
                path: header_path(paths),
                deleted: false,
                lines: Vec::new(),
            });
            header = true;
        } else if header && (line.starts_with("deleted file mode") || line == "+++ /dev/null") {
            entry.iter_mut().for_each(|e| e.deleted = true);
        } else if line.starts_with("@@ ") {
            header = false;
            // `@@ -a,b +c,d @@`: `d` lines from line `c` on; `d` is 1 when left out.
            let new = line.split_whitespace().nth(2).unwrap_or("+0");
            let new = new.trim_start_matches('+');
            let (start, count) = new.split_once(',').unwrap_or((new, "1"));
            let start: usize = start.parse().unwrap_or(0);
            let count: usize = count.parse().unwrap_or(1);
            let range = match count {
                0 => (start.max(1), start.max(1)),
                n => (start, start + n - 1),
            };
            entry.iter_mut().for_each(|e| e.lines.push(range));
        }
    }
    finish(entry);
    changes
}

/// The path of a `diff --git a/<path> b/<path>` header (the rest after `diff --git `): with
/// `--no-renames` both sides are the same path. ponytail: a path git still quotes (a tab, a
/// newline or a quote in the name) is skipped.
fn header_path(paths: &str) -> Option<String> {
    let len = paths.len().checked_sub(5).filter(|n| n % 2 == 0)? / 2;
    let a = paths.strip_prefix("a/")?.get(..len)?;
    let b = paths.get(2 + len..)?.strip_prefix(" b/")?;
    (a == b).then(|| a.to_string())
}

/// The mirrors a server-mode manifest lists, which `git` may read although another user owns
/// them (ADR 0011). Empty locally: nothing there is trusted.
static TRUSTED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Trusts the mirror at `path` for the `git` calls Meada makes in it: they pass
/// `-c safe.directory=<path>`. Only for a path the manifest lists, never `*`.
pub fn trust(path: &Path) {
    if let Ok(real) = path.canonicalize() {
        let mut trusted = TRUSTED.lock().unwrap_or_else(|e| e.into_inner());
        if !trusted.contains(&real) {
            trusted.push(real);
        }
    }
}

/// The directories holding mirrors pulled from a store (`meada snapshot pull`, ADR 0012):
/// hostile input, read with no system or global git configuration.
static ISOLATED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Every `git` call Meada makes in a directory under `dir` runs with `GIT_CONFIG_NOSYSTEM=1`
/// and no global configuration, so only the repository's own configuration (the minimal one
/// the pull wrote) applies, and with `core.fsmonitor=false`, so no fsmonitor runs either.
pub fn isolate(dir: &Path) {
    if let Ok(real) = dir.canonicalize() {
        let mut isolated = ISOLATED.lock().unwrap_or_else(|e| e.into_inner());
        if !isolated.contains(&real) {
            isolated.push(real);
        }
    }
}

/// `git` in `dir`: the one place every call goes through, so the one place `safe.directory`
/// and the isolation of pulled mirrors are added.
fn command(dir: &Path) -> Command {
    let mut git = Command::new("git");
    let real = dir.canonicalize().ok();
    let isolated = ISOLATED.lock().unwrap_or_else(|e| e.into_inner());
    if (real.as_ref()).is_some_and(|real| isolated.iter().any(|d| real.starts_with(d))) {
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        git.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null)
            .args(["-c", "core.fsmonitor=false"]);
    }
    drop(isolated);
    let trusted = TRUSTED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(real) = real.filter(|real| trusted.contains(real)) {
        let mut option = std::ffi::OsString::from("safe.directory=");
        option.push(real);
        git.arg("-c").arg(option);
    }
    git.arg("-C").arg(dir);
    git
}

/// Whether `git` in the trusted mirror `dir` still refuses it for its owner: before git 2.38 it
/// ignores `safe.directory` given on the command line.
pub fn refused_as_dubious(dir: &Path) -> bool {
    let trusted = dir
        .canonicalize()
        .is_ok_and(|real| (TRUSTED.lock().unwrap_or_else(|e| e.into_inner())).contains(&real));
    trusted
        && (command(dir).args(["rev-parse", "--show-toplevel"]).output())
            .is_ok_and(|out| is_dubious(&String::from_utf8_lossy(&out.stderr)))
}

/// Whether `stderr` is git's refusal of a directory another user owns.
fn is_dubious(stderr: &str) -> bool {
    stderr.contains("dubious ownership")
}

/// `git` in `dir`, its standard output, or its error output as the error.
fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = command(dir).args(args).output()?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `run`'s output trimmed, or `None` when it fails.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    run(dir, args).ok().map(|out| out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        Change, Changes, command, is_dubious, isolate, normalize_remote, relative, trust,
        uncommitted,
    };
    use std::path::{Path, PathBuf};

    /// A new repository under the temporary directory, named for the test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meada-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["-c", "init.defaultBranch=main", "init", "-q"]);
        dir
    }

    /// `git` in `dir`, isolated from the user's configuration, with a fixed author.
    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Meada")
            .env("GIT_AUTHOR_EMAIL", "fixtures@example.com")
            .env("GIT_COMMITTER_NAME", "Meada")
            .env("GIT_COMMITTER_EMAIL", "fixtures@example.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn commit(dir: &Path, message: &str) {
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", message]);
    }

    fn write(dir: &Path, path: &str, text: &[u8]) {
        std::fs::write(dir.join(path), text).unwrap();
    }

    fn range<'a>(base: &'a str, head: &'a str) -> Changes<'a> {
        Changes::Range { base, head }
    }

    fn sorted(mut changes: Vec<(String, Change)>) -> Vec<(String, Change)> {
        changes.sort_by(|a, b| a.0.cmp(&b.0));
        changes
    }

    #[test]
    fn a_range_gives_the_lines_of_its_head() {
        let dir = scratch("range-lines");
        write(&dir, "kept.ts", b"a\nb\nc\nd\n");
        write(&dir, "gone.ts", b"x\n");
        commit(&dir, "base");
        write(&dir, "kept.ts", b"a\nB\nc\nd\ne\n");
        std::fs::remove_file(dir.join("gone.ts")).unwrap();
        write(&dir, "new.ts", b"1\n2\n3\n");
        commit(&dir, "head");
        let changes = sorted(range("HEAD~1", "HEAD").read(&dir).unwrap());
        assert_eq!(
            changes,
            [
                ("gone.ts".to_string(), Change::Deleted),
                ("kept.ts".to_string(), Change::Lines(vec![(2, 2), (5, 5)])),
                ("new.ts".to_string(), Change::Lines(vec![(1, 3)])),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_change_with_no_hunk_is_a_whole_file() {
        let dir = scratch("no-hunk");
        write(&dir, "image.bin", b"\x00\x01\x02");
        write(&dir, "run.sh", b"echo\n");
        write(&dir, "gone.bin", b"\x00\x09");
        write(&dir, "gone-empty", b"");
        commit(&dir, "base");
        write(&dir, "image.bin", b"\x00\x03\x04");
        git(&dir, &["update-index", "--chmod=+x", "run.sh"]);
        std::fs::set_permissions(
            dir.join("run.sh"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        std::fs::remove_file(dir.join("gone.bin")).unwrap();
        std::fs::remove_file(dir.join("gone-empty")).unwrap();
        write(&dir, "new-empty", b"");
        git(&dir, &["add", "new-empty"]);
        let whole = [
            ("gone-empty".to_string(), Change::Deleted),
            ("gone.bin".to_string(), Change::Deleted),
            ("image.bin".to_string(), Change::Untracked),
            ("new-empty".to_string(), Change::Untracked),
            ("run.sh".to_string(), Change::Untracked),
        ];
        assert_eq!(sorted(Changes::Uncommitted.read(&dir).unwrap()), whole);
        // The MCP tool's reading keeps dropping them, so a chmod moves no answer.
        assert_eq!(uncommitted(&dir).unwrap(), []);
        commit(&dir, "head");
        assert_eq!(sorted(range("HEAD~1", "HEAD").read(&dir).unwrap()), whole);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_with_a_space_is_read() {
        let dir = scratch("space");
        write(&dir, "my file.ts", b"a\n");
        write(&dir, "a\u{e7}\u{e3}o.ts", b"a\n");
        commit(&dir, "base");
        write(&dir, "my file.ts", b"b\n");
        write(&dir, "a\u{e7}\u{e3}o.ts", b"b\n");
        commit(&dir, "head");
        let lines = || Change::Lines(vec![(1, 1)]);
        let expected = [
            ("a\u{e7}\u{e3}o.ts".to_string(), lines()),
            ("my file.ts".to_string(), lines()),
        ];
        assert_eq!(
            sorted(range("HEAD~1", "HEAD").read(&dir).unwrap()),
            expected
        );
        let files = ["my file.ts".to_string(), "a\u{e7}\u{e3}o.ts".to_string()];
        assert_eq!(
            sorted(Changes::Files(&files).read(&dir).unwrap()),
            [
                ("a\u{e7}\u{e3}o.ts".to_string(), Change::Untracked),
                ("my file.ts".to_string(), Change::Untracked),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_range_changes_nothing() {
        let dir = scratch("empty-range");
        write(&dir, "a.ts", b"a\n");
        commit(&dir, "base");
        assert_eq!(range("HEAD", "HEAD").read(&dir).unwrap(), []);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_mark_a_missing_path_deleted() {
        let dir = scratch("files-missing");
        write(&dir, "a.ts", b"a\n");
        let files = ["a.ts".to_string(), "./sub/../gone.ts".to_string()];
        assert_eq!(
            Changes::Files(&files).read(&dir).unwrap(),
            [
                ("a.ts".to_string(), Change::Untracked),
                ("gone.ts".to_string(), Change::Deleted),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_range_refuses_a_revision_shaped_as_an_option() {
        let dir = scratch("range-option");
        write(&dir, "a.ts", b"a\n");
        commit(&dir, "base");
        let written = dir.join("written");
        let base = format!("--output={}", written.display());
        let refused = range(&base, "HEAD").read(&dir).err().map(|e| e.to_string());
        assert!(refused.is_some_and(|e| e.contains("`--changes` takes")));
        assert!(!written.exists(), "git took the base as an option");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_refuse_a_directory() {
        let dir = scratch("files-directory");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        write(&dir, "sub/a.ts", b"a\n");
        let files = ["sub".to_string()];
        let refused = Changes::Files(&files)
            .read(&dir)
            .err()
            .map(|e| e.to_string());
        assert!(refused.is_some_and(|e| e.contains("`sub` is a directory")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_refuse_a_path_outside() {
        let dir = scratch("files-outside");
        for path in ["../x.ts", "/etc/hosts", "a/../../x.ts", ""] {
            let files = [path.to_string()];
            let refused = Changes::Files(&files)
                .read(&dir)
                .err()
                .map(|e| e.to_string());
            assert!(
                refused
                    .as_ref()
                    .is_some_and(|e| e.contains("not a path inside the repository")),
                "{path:?}: {refused:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_refusal_of_another_owner_is_recognized() {
        let stderr = "fatal: detected dubious ownership in repository at '/srv/web'\n\
                      To add an exception for this directory, call:\n";
        assert!(is_dubious(stderr));
        assert!(!is_dubious("fatal: not a git repository\n"));
    }

    #[test]
    fn only_a_trusted_mirror_gets_safe_directory() {
        let dir = std::env::temp_dir().join(format!("meada-trust-{}", std::process::id()));
        let (listed, other) = (dir.join("listed"), dir.join("other"));
        std::fs::create_dir_all(&listed).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let args = |dir: &std::path::Path| -> Vec<String> {
            let git = command(dir);
            (git.get_args())
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(args(&listed)[0], "-C");
        trust(&listed);
        let real = listed.canonicalize().unwrap();
        let option = format!("safe.directory={}", real.display());
        assert_eq!(args(&listed)[..2], ["-c".to_string(), option]);
        assert_eq!(args(&other)[0], "-C");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_under_an_isolated_directory_reads_no_system_or_global_configuration() {
        let dir = std::env::temp_dir().join(format!("meada-isolate-{}", std::process::id()));
        let (pulled, other) = (dir.join("pulled"), dir.join("other"));
        std::fs::create_dir_all(pulled.join("mirror")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let envs = |dir: &std::path::Path| -> Vec<(String, String)> {
            let git = command(dir);
            let mut envs: Vec<_> = (git.get_envs())
                .map(|(k, v)| {
                    let v = v.map(|v| v.to_string_lossy().into_owned());
                    (k.to_string_lossy().into_owned(), v.unwrap_or_default())
                })
                .collect();
            envs.sort();
            envs
        };
        assert!(envs(&other).is_empty());
        isolate(&pulled);
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let expected = [
            ("GIT_CONFIG_GLOBAL".to_string(), null.to_string()),
            ("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string()),
        ];
        assert_eq!(envs(&pulled.join("mirror")), expected);
        let git = command(&pulled.join("mirror"));
        let args: Vec<_> = git.get_args().map(|a| a.to_string_lossy()).collect();
        assert_eq!(args[..2], ["-c", "core.fsmonitor=false"]);
        assert!(envs(&other).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_path_folds_against_its_directory() {
        assert_eq!(relative("", "core"), Some("core".into()));
        assert_eq!(relative("app/web", "../core"), Some("app/core".into()));
        assert_eq!(relative("app", "./x/./y"), Some("app/x/y".into()));
        assert_eq!(relative("app", "."), Some("app".into()));
        assert_eq!(relative("app", ".."), Some("".into()));
    }

    #[test]
    fn a_path_that_leaves_the_repository_keeps_its_dotdot_segments() {
        assert_eq!(relative("app", "../../x"), Some("../x".into()));
        assert_eq!(relative("", "../x"), Some("../x".into()));
    }

    #[test]
    fn an_absolute_or_backslashed_path_folds_to_nothing() {
        assert_eq!(relative("app", "/etc"), None);
        assert_eq!(relative("app", "a\\b"), None);
    }

    #[test]
    fn remote_forms_of_one_repository_are_equal() {
        for url in [
            "git@github.com:Acme/Web.git",
            "ssh://git@github.com/acme/web.git",
            "ssh://git@github.com:22/acme/web",
            "https://github.com/acme/web.git",
            "https://user:token@GitHub.com/Acme/Web/",
            "http://github.com/acme/web",
            "  https://github.com/acme/web.git\n",
        ] {
            assert_eq!(normalize_remote(url), "github.com/acme/web", "{url}");
        }
    }

    #[test]
    fn nested_paths_and_other_hosts_are_kept() {
        assert_eq!(
            normalize_remote("git@gitlab.example.com:group/sub/api.git"),
            "gitlab.example.com/group/sub/api"
        );
        assert_eq!(
            normalize_remote("https://gitlab.example.com/group/sub/api"),
            "gitlab.example.com/group/sub/api"
        );
    }
}
