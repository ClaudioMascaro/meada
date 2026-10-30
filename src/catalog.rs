//! The catalog's section (#341, ADR 0021 contract 1, feature `workspaces`): `[catalog]` names the
//! repositories an administrator allows the role check to list besides the ones the workspace
//! selects, and how many it lists at most. Coverage is a fact the job reads from GitHub and
//! entitlement is another: nothing in this file grants a repository to anyone.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::modules::Problem;
use crate::msg::Msg;

// ponytail: the most repositories `max_repositories` may allow.
const MAX_REPOSITORIES: i64 = 5000;

/// `[catalog]` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    allow: Option<Vec<String>>,
    max_repositories: Option<i64>,
}

/// One entry of `allow`: every repository of an owner, or one repository. Lowercase, as GitHub
/// compares names.
#[derive(Clone, Debug, PartialEq)]
enum Pattern {
    Owner(String),
    Repository(String, String),
}

/// The validated section.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    allow: Vec<Pattern>,
    /// The most candidates the catalog holds besides the repositories the workspace selects.
    pub max_repositories: usize,
}

impl File {
    /// The section checked against `host`, `[oidc.github] host`: the first problem.
    pub fn validate(self, host: &str) -> Result<Config, Problem> {
        let Some(allow) = self.allow else {
            return Err(("catalog", Msg::KeyMissing("catalog.allow").text()));
        };
        let bad = || ("catalog.allow", Msg::CatalogAllow(host).text());
        if allow.is_empty() {
            return Err(bad());
        }
        let mut patterns: Vec<Pattern> = Vec::new();
        for text in &allow {
            let pattern = pattern(text, host).ok_or_else(bad)?;
            if patterns.contains(&pattern) {
                return Err(bad());
            }
            patterns.push(pattern);
        }
        let key = "catalog.max_repositories";
        let Some(max) = self.max_repositories else {
            return Err(("catalog", Msg::KeyMissing(key).text()));
        };
        let max = crate::modules::at_least(key, Some(max), 1, 1)?;
        match i64::try_from(max) {
            Ok(max) if max <= MAX_REPOSITORIES => Ok(Config {
                allow: patterns,
                max_repositories: max as usize,
            }),
            _ => Err((key, Msg::AtMost(MAX_REPOSITORIES as u64).text())),
        }
    }
}

/// `text` as `<host>/<owner>/*` or `<host>/<owner>/<repo>`, compared exactly: lowercase, the
/// owner of ASCII letters, digits, `-` and `_`, the repository those and `.`, never `.` or `..`.
fn pattern(text: &str, host: &str) -> Option<Pattern> {
    let rest = text.strip_prefix(host)?.strip_prefix('/')?;
    let (owner, repo) = rest.split_once('/')?;
    let owner_ok = !owner.is_empty()
        && (owner.chars())
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_".contains(c));
    if !owner_ok {
        return None;
    }
    if repo == "*" {
        return Some(Pattern::Owner(owner.to_string()));
    }
    let repo_ok = !matches!(repo, "" | "." | "..")
        && !repo.ends_with(".git")
        && (repo.chars())
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c));
    repo_ok.then(|| Pattern::Repository(owner.to_string(), repo.to_string()))
}

impl Config {
    /// Whether `owner/repo` (lowercase) is allowed.
    pub fn allows(&self, owner: &str, repo: &str) -> bool {
        self.allow.iter().any(|p| match p {
            Pattern::Owner(o) => o == owner,
            Pattern::Repository(o, r) => o == owner && r == repo,
        })
    }

    /// The owners some pattern names: an installation on any other owner is not listed.
    pub fn owners(&self) -> BTreeSet<&str> {
        fn owner(p: &Pattern) -> &str {
            match p {
                Pattern::Owner(o) | Pattern::Repository(o, _) => o,
            }
        }
        self.allow.iter().map(owner).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(toml: &str) -> File {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn a_section_names_patterns_on_the_host_and_a_cap() {
        let config = file(
            "allow = [\"example.com/acme/*\", \"example.com/other/tool\"]\nmax_repositories = 40",
        )
        .validate("example.com")
        .unwrap();
        assert_eq!(config.max_repositories, 40);
        assert!(config.allows("acme", "anything"));
        assert!(config.allows("other", "tool"));
        assert!(!config.allows("other", "more"));
        assert!(!config.allows("elsewhere", "tool"));
        assert_eq!(
            config.owners().into_iter().collect::<Vec<_>>(),
            ["acme", "other"]
        );
    }

    #[test]
    fn a_bad_section_is_refused_at_its_key() {
        let key = |toml: &str| file(toml).validate("example.com").unwrap_err().0;
        let ok = "max_repositories = 5\nallow = ";
        assert_eq!(key("max_repositories = 5"), "catalog");
        assert_eq!(key("allow = [\"example.com/a/*\"]"), "catalog");
        for allow in [
            "[]",
            "[\"example.com/a/*\", \"example.com/a/*\"]",
            "[\"github.com/a/*\"]",
            "[\"example.com/*\"]",
            "[\"example.com/A/*\"]",
            "[\"example.com/a/B\"]",
            "[\"example.com/a/b/c\"]",
            "[\"example.com/a/b.git\"]",
            "[\"example.com/a/..\"]",
            "[\"example.com//x\"]",
            "[\"example.com/a b/x\"]",
        ] {
            assert_eq!(key(&format!("{ok}{allow}")), "catalog.allow", "{allow}");
        }
        for cap in ["0", "-1", "5001"] {
            let toml = format!("allow = [\"example.com/a/*\"]\nmax_repositories = {cap}");
            assert_eq!(key(&toml), "catalog.max_repositories", "{cap}");
        }
        assert!(toml::from_str::<File>("allow = []\nmax_repositories = 1\nextra = 1").is_err());
    }
}
