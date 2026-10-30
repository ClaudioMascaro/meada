//! Named workspaces (ADR 0020, ADR 0021): the pure rules of the Y/A/B matrix and the entries of a
//! named version. What a caller may select, what a non-operator sees and what an answer shows are
//! decided here from the sets the binary reads (the permissions set's eligible identities, the
//! token's grant), so the console's server and the browser, compiled to WebAssembly, give one
//! answer. `tests/fixtures/named-workspaces.json` holds the table both are tested against.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value as Json;

use crate::authorize::Grant;
use crate::ids::named_version_id;
use crate::manifest::{EntryFile, validate_named};
use crate::msg::Msg;

// ponytail: the most bytes of a named workspace's manifest text, the longest name and the
// longest description, in characters.
pub const TEXT_MAX: usize = 64 << 10;
pub const NAME_MAX: usize = 80;
pub const DESCRIPTION_MAX: usize = 500;

/// Whether `text` is a plain name of `min` to `max` characters: no control character and no
/// whitespace at either end.
fn text_ok(text: &str, min: usize, max: usize) -> bool {
    let chars = text.chars().count();
    let padded = text.chars().next().is_some_and(char::is_whitespace)
        || text.chars().next_back().is_some_and(char::is_whitespace);
    (min..=max).contains(&chars) && !padded && !text.chars().any(char::is_control)
}

/// The problem with `name` (1 to `NAME_MAX` characters), if any.
pub fn name_problem(name: &str) -> Option<String> {
    (!text_ok(name, 1, NAME_MAX)).then(|| Msg::NamedNameBad(NAME_MAX).text())
}

/// The problem with `description` (0 to `DESCRIPTION_MAX` characters), if any.
pub fn description_problem(description: &str) -> Option<String> {
    (!text_ok(description, 0, DESCRIPTION_MAX))
        .then(|| Msg::NamedDescriptionBad(DESCRIPTION_MAX).text())
}

/// One entry of a named version document: `repo` and then the entry's blocks (`entry_blocks`),
/// with no `path`, `owner` or `added_by`.
pub fn entry(e: &EntryFile, blocks: Json) -> Json {
    let mut entry = serde_json::Map::new();
    entry.insert("repo".into(), e.repo.as_str().into());
    if let Json::Object(blocks) = blocks {
        entry.extend(blocks);
    }
    Json::Object(entry)
}

/// The entries of the named text `src` as a version document holds them, or the problems
/// `validate_named` finds.
pub fn version_entries(file: &str, src: &str, max: usize) -> Result<Vec<Json>, Vec<String>> {
    let entries = validate_named(file, src, max)?;
    Ok((entries.iter().zip(crate::ids::entry_blocks(src)))
        .map(|(e, blocks)| entry(e, blocks))
        .collect())
}

/// The version id of named entries as a document holds them: the hash of their `repo` and
/// blocks, in order.
pub fn version_id(entries: &[Json]) -> String {
    let blocks: Vec<(String, Json)> = (entries.iter())
        .map(|e| {
            let mut blocks = e.clone();
            let repo = blocks
                .as_object_mut()
                .and_then(|o| o.remove("repo"))
                .and_then(|r| r.as_str().map(str::to_string))
                .unwrap_or_default();
            (repo, blocks)
        })
        .collect();
    let refs: Vec<(&str, &Json)> = blocks.iter().map(|(r, b)| (r.as_str(), b)).collect();
    named_version_id(&refs)
}

/// The identities of the entries of a named version document, in order.
pub fn identities(entries: &[Json]) -> Vec<String> {
    (entries.iter())
        .filter_map(|e| e["repo"].as_str().map(str::to_string))
        .collect()
}

/// What creating or extending a workspace from a selection gives (ADR 0020).
#[derive(Debug, PartialEq, Serialize)]
pub struct Creation {
    pub allowed: bool,
    /// The refused identities the caller's grant reads, in the selection's order.
    pub named: Vec<String>,
    /// How many refused identities the caller's grant does not read: counted, never named.
    pub hidden: usize,
}

/// Whether the caller may select `selected`: every identity must be in `eligible`, the
/// identities its login holds an accepted role on. A refusal names the identities `grant` reads
/// and counts the others, so a refusal is no oracle on what it cannot read.
pub fn creation(selected: &[String], eligible: &BTreeSet<String>, grant: &Grant) -> Creation {
    let refused: Vec<&String> = selected.iter().filter(|i| !eligible.contains(*i)).collect();
    Creation {
        allowed: refused.is_empty(),
        named: (refused.iter())
            .filter(|i| grant.reads(i))
            .map(|i| (*i).clone())
            .collect(),
        hidden: refused.iter().filter(|i| !grant.reads(i)).count(),
    }
}

/// Whether a caller sees a workspace of `entries` at all (the discovery rule of ADR 0020): its
/// operators and the platform administrator always, anyone else only when its grant reads every
/// entry. Names, descriptions and counts are not bounded by repository grants, so a partly
/// readable workspace is not shown filtered.
pub fn visible(operates: bool, entries: &[String], grant: &Grant) -> bool {
    operates || entries.iter().all(|e| grant.reads(e))
}

/// The entries `grant` reads, in order.
pub fn readable(entries: &[String], grant: &Grant) -> Vec<String> {
    entries.iter().filter(|e| grant.reads(e)).cloned().collect()
}

/// Whether an answer about `identity` in a workspace of `entries` is given: the workspace is
/// visible, holds the identity and the grant reads it.
pub fn answers(operates: bool, entries: &[String], grant: &Grant, identity: &str) -> bool {
    visible(operates, entries, grant)
        && entries.iter().any(|e| e == identity)
        && grant.reads(identity)
}

/// A link from `from` to `to` is shown only when both endpoints are answered.
pub fn shown(operates: bool, entries: &[String], grant: &Grant, from: &str, to: &str) -> bool {
    answers(operates, entries, grant, from) && answers(operates, entries, grant, to)
}

/// The sponsor rule (ADR 0021): the entries no operator holds an accepted role on, where each
/// element of `operators` is the identities one operator is eligible for. An excluded entry is
/// not fetched on its workspace's account.
pub fn unsponsored(entries: &[String], operators: &[BTreeSet<String>]) -> Vec<String> {
    (entries.iter())
        .filter(|e| !operators.iter().any(|eligible| eligible.contains(*e)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn names_and_descriptions_are_bounded_and_plain() {
        assert!(name_problem("Payments").is_none());
        for bad in ["", " x", "x ", "a\nb", &"x".repeat(NAME_MAX + 1)] {
            assert!(name_problem(bad).is_some(), "{bad:?}");
        }
        assert!(description_problem("").is_none());
        assert!(description_problem("\tx").is_some());
        assert!(description_problem(&"x".repeat(DESCRIPTION_MAX + 1)).is_some());
    }

    #[test]
    fn a_refusal_names_what_the_grant_reads_and_counts_the_rest() {
        let eligible = set(&["y", "a"]);
        let grant = Grant::Only(set(&["y", "a", "c"]));
        let got = creation(&list(&["y", "a", "b", "c"]), &eligible, &grant);
        assert_eq!(
            got,
            Creation {
                allowed: false,
                named: list(&["c"]),
                hidden: 1
            }
        );
        assert!(creation(&list(&["y", "a"]), &eligible, &grant).allowed);
    }

    #[test]
    fn a_workspace_is_visible_to_a_non_operator_only_whole() {
        let grant = Grant::Only(set(&["y", "a"]));
        assert!(visible(false, &list(&["y", "a"]), &grant));
        assert!(!visible(false, &list(&["y", "a", "b"]), &grant));
        assert!(visible(true, &list(&["y", "a", "b"]), &grant));
        assert_eq!(readable(&list(&["y", "a", "b"]), &grant), list(&["y", "a"]));
        assert!(!answers(true, &list(&["y", "b"]), &grant, "b"));
        assert!(!shown(false, &list(&["y", "a"]), &grant, "y", "b"));
    }

    #[test]
    fn a_named_entry_refuses_what_the_platform_owns() {
        let text = |body: &str| validate_named("w", body, 10);
        let ok = "[[repository]]\nrepo = \"h/o/y\"\n[repository.serve]\nhosts = [\"y.example\"]\n";
        assert_eq!(text(ok).unwrap().len(), 1);
        for (bad, key) in [
            ("[[repository]]\nrepo = \"h/o/y\"\npath = \"p\"\n", "path"),
            ("[[repository]]\nrepo = \"h/o/y\"\nowner = \"g\"\n", "owner"),
            (
                "[[repository]]\nrepo = \"h/o/y\"\nadded_by = \"a\"\n",
                "added_by",
            ),
            (
                "[[repository]]\nrepo = \"h/o/y\"\n[repository.serve]\ndocuments = [\"d\"]\n",
                "documents",
            ),
        ] {
            let problems = text(bad).err().unwrap_or_default();
            assert!(
                problems
                    .iter()
                    .any(|p| p.contains("is reserved") && p.contains(key)),
                "{key}: {problems:?}"
            );
        }
        let env = "[[repository]]\nrepo = \"h/o/y\"\n[[repository.peer]]\nrepo = \"h/o/a\"\n\
                   base_url_env = [\"A_URL\", \"A=https://x\"]\n";
        assert!(text(env).is_err());
        let twice = "[[repository]]\nrepo = \"h/o/y\"\n[[repository]]\nrepo = \"h/o/y\"\n";
        assert!(text(twice).is_err());
        assert!(validate_named("w", ok, 0).is_err());
    }

    #[test]
    fn a_named_version_id_ignores_what_a_named_entry_does_not_hold() {
        let a = version_entries("w", "[[repository]]\nrepo = \"h/o/y\"\n", 5).unwrap();
        let b = version_entries("w", "[[repository]]\n\nrepo = \"h/o/y\"\n", 5).unwrap();
        assert_eq!(version_id(&a), version_id(&b));
        assert_eq!(identities(&a), ["h/o/y"]);
        let c = version_entries("w", "[[repository]]\nrepo = \"h/o/a\"\n", 5).unwrap();
        assert_ne!(version_id(&a), version_id(&c));
    }
}
