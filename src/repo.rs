//! Repository root and identity, read through the `git` command line.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// The top level of the git repository containing `dir`, or `dir` itself outside git.
pub fn root(dir: &Path) -> Result<PathBuf> {
    match git(dir, &["rev-parse", "--show-toplevel"]) {
        Some(top) => Ok(PathBuf::from(top)),
        None => Ok(dir.canonicalize()?),
    }
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

/// `git` in `dir`, its standard output, or its error output as the error.
fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
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

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_remote;

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
