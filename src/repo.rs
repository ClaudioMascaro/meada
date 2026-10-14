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
