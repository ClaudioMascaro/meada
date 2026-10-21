//! Repository root and identity, read through the `git` command line.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use crate::Result;

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

/// What a path of the working tree changed against `HEAD`.
pub enum Change {
    /// Inclusive 1-based line ranges of the file as it is now; a deletion touches the line
    /// before it.
    Lines(Vec<(usize, usize)>),
    /// An untracked file: all of it.
    Untracked,
}

/// Every repository-relative path whose working-tree content differs from `HEAD`, tracked or
/// untracked (ignored files are left out, deleted files too, since nothing of theirs is left).
/// Before the first commit, every file counts as untracked.
pub fn uncommitted(root: &Path) -> Result<Vec<(String, Change)>> {
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
    // Explicit prefixes and no external diff, whatever the user's git configuration says.
    let diff = run(
        root,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "HEAD",
            "--no-renames",
            "--no-ext-diff",
            "--no-color",
            "--unified=0",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ],
    )?;
    let mut changes: Vec<(String, Change)> = Vec::new();
    let mut header = false;
    let mut path: Option<String> = None;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            header = true;
            path = None;
        } else if header && let Some(to) = line.strip_prefix("+++ ") {
            // `/dev/null` for a deleted file; a name with a space ends with a tab. ponytail: a
            // path git still quotes (a tab, a newline or a quote in the name) is skipped.
            let to = to.strip_suffix('\t').unwrap_or(to);
            path = to.strip_prefix("b/").map(str::to_string);
        } else if line.starts_with("@@ ") {
            header = false;
            let Some(path) = &path else { continue };
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
            match changes.last_mut() {
                Some((p, Change::Lines(ranges))) if p == path => ranges.push(range),
                _ => changes.push((path.clone(), Change::Lines(vec![range]))),
            }
        }
    }
    changes.extend(untracked(&[])?);
    Ok(changes)
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

/// `git` in `dir`: the one place every call goes through, so the one place `safe.directory`
/// is added.
fn command(dir: &Path) -> Command {
    let mut git = Command::new("git");
    let real = dir.canonicalize().ok();
    let trusted = TRUSTED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(real) = real.filter(|real| trusted.contains(real)) {
        let mut option = std::ffi::OsString::from("safe.directory=");
        option.push(real);
        git.arg("-c").arg(option);
    }
    git.arg("-C").arg(dir);
    git
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
    use super::{command, normalize_remote, relative, trust};

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
