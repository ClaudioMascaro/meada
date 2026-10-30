//! The effective configuration of a named workspace's entries (ADR 0021, contract 6): what a
//! repository declares in one context, decided by layers with their source shown and never
//! merged. A repository's own `meada.toml` wins whole (ADR 0011), so the context's blocks for it
//! are ignored and say so; with no file of its own the context's blocks apply; with neither the
//! repository declares nothing. The function is pure over the entries and a few facts about
//! each repository's bundle, so the console's server, the browser (compiled to WebAssembly) and
//! the proposal interface decide alike. It reads no mirror: the console supplies the facts from
//! the generation it holds, and the view it builds is annotation, never part of a manifest's
//! canonical text or of any id.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// What the generation a console holds says about one repository. A repository the generation
/// has no bundle of is every field false.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Facts {
    /// A held generation has a bundle of the repository.
    pub held: bool,
    /// The bundle's mirror has a `meada.toml` of its own.
    pub own: bool,
    /// The bundle indexes provider documents: its ingestion configuration declares `[serve]`
    /// (the repository's own file, else the deployment entry's blocks).
    pub documents: bool,
    /// The newest permissions set excludes the entry from this workspace (`no_sponsor`).
    pub excluded: bool,
}

/// Where an entry's effective configuration comes from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The repository's own `meada.toml`, whole.
    Repository,
    /// The context's blocks for the entry.
    Context,
    /// Neither declares anything.
    None,
}

/// Whether the entry's repository is part of what the generation indexed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Indexed,
    PendingIngestion,
    Excluded,
}

/// One observation about an entry's configuration: a stable `code` the front end words in
/// either language, and the `detail` it is about (a block, a peer) when it has one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn diagnostic(code: &'static str, detail: Option<String>) -> Diagnostic {
    Diagnostic { code, detail }
}

/// The effective configuration of one entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Effective {
    pub repo: String,
    pub source: Source,
    /// The context's blocks that apply: those of the entry when `source` is `context`, else
    /// empty. A repository's own file is not repeated here (the export shows what it declares).
    pub blocks: Json,
    /// The context's blocks the repository's own file overrides, worded as `status` words them.
    pub ignored: Vec<String>,
    pub coverage: Coverage,
    pub diagnostics: Vec<Diagnostic>,
}

/// The block labels of an entry's `blocks` (an entry without `repo`), in the order `status`
/// names them: `[repository.serve]`, `[[repository.resource]] <kind> <id>`,
/// `[[repository.peer]] <repo>`.
pub fn labels(blocks: &Json) -> Vec<String> {
    let text = |value: &Json, key: &str| value[key].as_str().unwrap_or_default().to_string();
    let list = |key: &str| blocks[key].as_array().cloned().unwrap_or_default();
    let serve = blocks
        .get("serve")
        .map(|_| "[repository.serve]".to_string());
    let resources = (list("resource").iter())
        .map(|r| {
            format!(
                "[[repository.resource]] {} {}",
                text(r, "kind"),
                text(r, "id")
            )
        })
        .collect::<Vec<_>>();
    let peers = (list("peer").iter())
        .map(|p| format!("[[repository.peer]] {}", text(p, "repo")))
        .collect::<Vec<_>>();
    serve.into_iter().chain(resources).chain(peers).collect()
}

/// The blocks of `entry` (a version document's entry): everything but `repo`.
fn blocks_of(entry: &Json) -> Json {
    let mut blocks = entry.clone();
    if let Some(map) = blocks.as_object_mut() {
        map.remove("repo");
    }
    blocks
}

/// The effective configuration of each of `entries` in order. The peers an entry names are
/// checked against `entries` alone: a peer outside the workspace is reported missing, never
/// discovered (ADR 0005), so the caller passes the entries it may read and nothing it may not.
/// `facts` is keyed by identity; an identity without facts is a repository no held generation
/// has.
pub fn effective(entries: &[Json], facts: &BTreeMap<String, Facts>) -> Vec<Effective> {
    let members: BTreeSet<&str> = (entries.iter())
        .filter_map(|e| e["repo"].as_str())
        .collect();
    let none = Facts::default();
    entries
        .iter()
        .map(|entry| {
            let repo = entry["repo"].as_str().unwrap_or_default().to_string();
            let blocks = blocks_of(entry);
            let declared = labels(&blocks);
            let facts = facts.get(&repo).unwrap_or(&none);
            let overridden = facts.held && facts.own;
            let source = match (overridden, declared.is_empty()) {
                (true, _) => Source::Repository,
                (false, false) => Source::Context,
                (false, true) => Source::None,
            };
            let mut diagnostics = Vec::new();
            if facts.excluded {
                diagnostics.push(diagnostic("excluded_no_sponsor", None));
            } else if !facts.held && !declared.is_empty() {
                diagnostics.push(diagnostic("unknown_until_ingested", None));
            }
            let (applied, ignored) = match overridden {
                true => (Json::Object(Default::default()), declared),
                false => (blocks, Vec::new()),
            };
            for label in &ignored {
                diagnostics.push(diagnostic("ignored_by_repository", Some(label.clone())));
            }
            for peer in applied["peer"].as_array().into_iter().flatten() {
                let name = peer["repo"].as_str().unwrap_or_default();
                if !members.contains(name) {
                    diagnostics.push(diagnostic("peer_missing", Some(name.to_string())));
                }
            }
            let hosts = applied["serve"]["hosts"].as_array().map_or(0, Vec::len);
            if facts.held && !facts.documents && hosts > 0 {
                diagnostics.push(diagnostic("documents_not_indexed", None));
            }
            let coverage = match (facts.excluded, facts.held) {
                (true, _) => Coverage::Excluded,
                (false, true) => Coverage::Indexed,
                (false, false) => Coverage::PendingIngestion,
            };
            Effective {
                repo,
                source,
                blocks: applied,
                ignored,
                coverage,
                diagnostics,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn facts(held: bool, own: bool, documents: bool, excluded: bool) -> Facts {
        Facts {
            held,
            own,
            documents,
            excluded,
        }
    }

    fn codes(effective: &Effective) -> Vec<&str> {
        effective.diagnostics.iter().map(|d| d.code).collect()
    }

    #[test]
    fn a_repository_file_wins_whole_and_the_context_blocks_are_ignored() {
        let entries = vec![json!({
            "repo": "github.com/acme/y",
            "serve": {"hosts": ["y.example"]},
            "resource": [{"kind": "postgres", "id": "orders"}],
        })];
        let held = BTreeMap::from([(
            "github.com/acme/y".to_string(),
            facts(true, true, true, false),
        )]);
        let [one] = &effective(&entries, &held)[..] else {
            panic!("one entry");
        };
        assert_eq!(one.source, Source::Repository);
        assert_eq!(one.blocks, json!({}));
        assert_eq!(
            one.ignored,
            [
                "[repository.serve]",
                "[[repository.resource]] postgres orders"
            ]
        );
        assert_eq!(codes(one), ["ignored_by_repository"; 2]);
        assert_eq!(one.coverage, Coverage::Indexed);
    }

    #[test]
    fn the_context_blocks_apply_when_the_mirror_has_no_file_of_its_own() {
        let entries = vec![
            json!({"repo": "github.com/acme/y", "peer": [{"repo": "github.com/acme/a"}]}),
            json!({"repo": "github.com/acme/a"}),
        ];
        let held = BTreeMap::from([
            (
                "github.com/acme/y".to_string(),
                facts(true, false, true, false),
            ),
            (
                "github.com/acme/a".to_string(),
                facts(true, false, true, false),
            ),
        ]);
        let found = effective(&entries, &held);
        assert_eq!(found[0].source, Source::Context);
        assert_eq!(
            found[0].blocks,
            json!({"peer": [{"repo": "github.com/acme/a"}]})
        );
        assert!(found[0].ignored.is_empty() && found[0].diagnostics.is_empty());
        assert_eq!(found[1].source, Source::None);
    }

    #[test]
    fn a_missing_peer_an_unheld_repository_and_an_exclusion_are_each_said() {
        let entries = vec![
            json!({"repo": "github.com/acme/y", "peer": [{"repo": "github.com/acme/b"}]}),
            json!({"repo": "github.com/acme/n", "serve": {"hosts": ["n.example"]}}),
            json!({"repo": "github.com/acme/x", "serve": {"hosts": ["x.example"]}}),
        ];
        let held = BTreeMap::from([
            (
                "github.com/acme/y".to_string(),
                facts(true, false, false, false),
            ),
            (
                "github.com/acme/x".to_string(),
                facts(false, false, false, true),
            ),
        ]);
        let found = effective(&entries, &held);
        assert_eq!(codes(&found[0]), ["peer_missing"]);
        assert_eq!(
            found[0].diagnostics[0].detail.as_deref(),
            Some("github.com/acme/b")
        );
        assert_eq!(codes(&found[1]), ["unknown_until_ingested"]);
        assert_eq!(found[1].coverage, Coverage::PendingIngestion);
        assert_eq!(codes(&found[2]), ["excluded_no_sponsor"]);
        assert_eq!(found[2].coverage, Coverage::Excluded);
    }

    #[test]
    fn serve_hosts_over_a_repository_with_no_documents_say_so() {
        let entries = vec![json!({"repo": "github.com/acme/y", "serve": {"hosts": ["y.example"]}})];
        let without = BTreeMap::from([(
            "github.com/acme/y".to_string(),
            facts(true, false, false, false),
        )]);
        assert_eq!(
            codes(&effective(&entries, &without)[0]),
            ["documents_not_indexed"]
        );
        let with = BTreeMap::from([(
            "github.com/acme/y".to_string(),
            facts(true, false, true, false),
        )]);
        assert!(effective(&entries, &with)[0].diagnostics.is_empty());
    }
}
