//! The CI contract kind (ADR 0028, spec #820): `ci::<repository identity>/<path>` links a
//! reusable pipeline to every CI job of the workspace that calls it. Both sides sit on the IaC
//! nodes the GitHub Actions reader stores (`iac::actions`): the provider on the file node of a
//! pipeline with `on.workflow_call`, the consumer on the CI job whose `uses` names it. A `uses`
//! writes no host, so the host is the consumer's own, and one whose repository is not in the
//! workspace holds no side at all. The Git ref is an attribute of the consumer, never part of
//! the identifier: it is shown as written and never resolved.

use std::collections::{HashMap, HashSet};

use meada_core::export as written;

use crate::contract::{ContractKind, Found, Hit, Level, Link, Search};
use crate::iac::{Node, actions};
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::resolve::Code;
use crate::workspace::Repository;

pub struct Ci;

const PREFIX: &str = "ci::";

/// A call to a reusable pipeline of another repository, as a CI job's `uses` writes it:
/// `<owner>/<repo>/.github/workflows/<file>@<ref>`. It holds no expression, so it is literal.
struct Call<'a> {
    owner: &'a str,
    repo: &'a str,
    path: &'a str,
    git_ref: &'a str,
}

impl<'a> Call<'a> {
    fn read(uses: &'a str) -> Option<Self> {
        // No name GitHub takes holds a control character, and one would break an answer's line.
        if uses.contains(char::is_control) {
            return None;
        }
        let (target, git_ref) = uses.split_once('@')?;
        let mut parts = target.splitn(3, '/');
        let (owner, repo, path) = (parts.next()?, parts.next()?, parts.next()?);
        let named = ![owner, repo, git_ref].iter().any(|part| part.is_empty());
        (named && owner != "." && actions::is_workflow(path)).then_some(Self {
            owner,
            repo,
            path,
            git_ref,
        })
    }
}

/// The host of a repository identity, which a `uses` leaves out: none for a repository without
/// `origin`, which names no peer.
fn host(identity: &str) -> Option<&str> {
    let (host, _) = identity.split_once('/')?;
    (host != "local").then_some(host)
}

/// A link's reason: the Git ref as written and the provider's indexed commit, with `ref
/// mismatch` only when the ref is a full commit id (40 or 64 hexadecimal digits) other than it.
fn reason(git_ref: &str, indexed: Option<&str>) -> String {
    let Some(commit) = indexed else {
        return format!("@{git_ref} (indexed commit unknown)");
    };
    let full = matches!(git_ref.len(), 40 | 64) && git_ref.bytes().all(|b| b.is_ascii_hexdigit());
    let mismatch = if full && !git_ref.eq_ignore_ascii_case(commit) {
        ", ref mismatch"
    } else {
        ""
    };
    let short = commit.get(..7).unwrap_or(commit);
    format!("@{git_ref} (indexed {short}){mismatch}")
}

/// The keys a call passes, as an answer shows them after it: ` with: a, b; secrets: inherit`.
fn passed(node: &Node) -> String {
    let Some(facts) = &node.actions else {
        return String::new();
    };
    let mut parts = Vec::new();
    if !facts.with.is_empty() {
        parts.push(format!("with: {}", facts.with.join(", ")));
    }
    match &facts.secrets {
        Some(written::Secrets::Inherit(_)) => parts.push("secrets: inherit".into()),
        Some(written::Secrets::Keys(keys)) => parts.push(format!("secrets: {}", keys.join(", "))),
        None => {}
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(" {}", parts.join("; "))
}

/// The node of the workspace at the side `site`, with its repository.
fn node_at<'e>(exports: &'e [Export], site: &str) -> Option<(&'e str, &'e Node)> {
    exports.iter().find_map(|export| {
        let rest = site.strip_prefix(export.repository.as_str())?;
        let rest = rest.strip_prefix("::")?;
        // `Node::identity` without writing it for every node: `<path>::<address>`.
        let at = |n: &Node| rest.strip_prefix(n.path.as_str())?.strip_prefix("::");
        let node =
            (export.iac.iter()).find(|n| actions::owns(n) && at(n) == Some(n.address.as_str()))?;
        Some((export.repository.as_str(), node))
    })
}

impl ContractKind for Ci {
    fn prefix(&self) -> &'static str {
        "ci"
    }
    fn extract(&self, _: &Code, _: &Indexed) -> Vec<Found> {
        Vec::new()
    }
    fn search_word(&self) -> Search {
        Search::Own("ci")
    }
    /// The CI nodes, not the contracts: `search` with the kind `contract` lists those.
    fn own_search(&self, exports: &[Export], _: &[Link], needle: &str) -> Vec<Hit> {
        (exports.iter())
            .flat_map(|export| actions::hits(&export.repository, &export.iac, needle))
            .collect()
    }
    fn in_default_search(&self) -> bool {
        false
    }
    /// Where a side is, as `context` takes it: the pipeline's path, or `<path>::<address>`.
    fn place(&self, repository: &str, site: &str) -> Option<String> {
        let rest = site.strip_prefix(repository)?.strip_prefix("::")?;
        Some(rest.strip_suffix("::.").unwrap_or(rest).to_string())
    }
    fn sides(&self) -> Option<(&'static str, &'static str)> {
        Some(("provider", "consumer"))
    }
    fn decidable(&self) -> bool {
        false
    }
    fn reads_commit(&self) -> bool {
        true
    }
    /// A CI job calls the pipeline it consumes: `trace` goes up from the provider to it, never
    /// down as from a writer to its readers.
    fn calls_from_definition(&self) -> bool {
        true
    }
    /// The sides of every repository, read from its CI nodes once the workspace is known: a
    /// call is a consumer only when the repository it names is one of the workspace.
    fn workspace_step(&self, exports: &mut [Export], repositories: &[Repository]) {
        let known: HashSet<&str> = repositories.iter().map(|r| r.identity.as_str()).collect();
        for export in exports.iter_mut() {
            let own = export.repository.as_str();
            for node in export.iac.iter().filter(|n| actions::owns(n)) {
                let Some(facts) = &node.actions else {
                    continue;
                };
                if node.address == "." && facts.reusable {
                    export.providers.push(Provider {
                        contract: format!("{PREFIX}{own}/{}", node.path),
                        site: node.identity(own),
                        ..Provider::default()
                    });
                }
                let called = (facts.uses.as_deref())
                    .filter(|_| node.ty == "job")
                    .and_then(Call::read);
                let (Some(call), Some(host)) = (called, host(own)) else {
                    continue;
                };
                let (owner, repo) = (call.owner.to_lowercase(), call.repo.to_lowercase());
                let repository = format!("{host}/{owner}/{repo}");
                if !known.contains(repository.as_str()) {
                    continue;
                }
                export.consumers.push(Consumer {
                    contract: Some(format!("{PREFIX}{repository}/{}", call.path)),
                    site: node.identity(own),
                    via: facts.uses.clone(),
                    shape: None,
                    method: None,
                    identity: None,
                    fragments: Vec::new(),
                    hops: Vec::new(),
                    root: None,
                });
            }
            crate::index::sort_providers(&mut export.providers);
            crate::index::sort_consumers(&mut export.consumers);
        }
    }
    fn link(
        &self,
        consumer: &Consumer,
        _: &Repository,
        providers: &[(&Repository, &Provider)],
        exports: &[Export],
    ) -> Vec<Link> {
        let (Some(contract), Some(call)) = (
            consumer.contract.as_deref(),
            consumer.via.as_deref().and_then(Call::read),
        ) else {
            return Vec::new();
        };
        let serving = providers.iter().filter(|(_, p)| p.contract == contract);
        serving
            .map(|(repository, provider)| {
                let indexed = (exports.iter())
                    .find(|e| e.repository == repository.identity)
                    .and_then(|e| e.commit.as_deref());
                Link::new(
                    consumer.site.clone(),
                    provider.site.clone(),
                    contract.into(),
                    Level::Exact,
                    reason(call.git_ref, indexed),
                    None,
                    None,
                    None,
                )
            })
            .collect()
    }
    fn context_at(
        &self,
        exports: &[Export],
        _: &[Repository],
        links: &[Link],
        site: &str,
        _: Option<&str>,
    ) -> Vec<String> {
        let ours = |contract: &str| contract.starts_with(PREFIX);
        let side = |word: &str, other: &str, link: &Link| {
            let (repository, node) = node_at(exports, other)?;
            let keys = if word == "consumer" {
                passed(node)
            } else {
                String::new()
            };
            Some(format!(
                "  {word}: {} [{}: {}]{keys}",
                crate::iac::at(repository, node),
                link.level.name(),
                link.reason
            ))
        };
        let mut lines = Vec::new();
        for export in exports {
            let provided =
                (export.providers.iter()).filter(|p| p.site == site && ours(&p.contract));
            for provider in provided {
                lines.push(format!("provides {}", provider.contract));
                let calling = (links.iter())
                    .filter(|l| l.provider == site && l.contract == provider.contract);
                lines.extend(calling.filter_map(|l| side("consumer", &l.consumer, l)));
            }
            for consumer in export.consumers.iter().filter(|c| c.site == site) {
                let Some(contract) = consumer.contract.as_deref().filter(|c| ours(c)) else {
                    continue;
                };
                let call = consumer.via.as_deref().and_then(Call::read);
                let git_ref = call.map_or(String::new(), |c| format!(" @{}", c.git_ref));
                let keys = node_at(exports, site).map_or(String::new(), |(_, n)| passed(n));
                lines.push(format!("consumes {contract}{git_ref}{keys}"));
                let called =
                    (links.iter()).filter(|l| l.consumer == site && l.contract == contract);
                lines.extend(called.filter_map(|l| side("provider", &l.provider, l)));
            }
        }
        lines
    }
    /// A call's Git ref and the keys it passes, on its consumer row.
    fn written(&self, nodes: &[Node], out: &mut written::Export) {
        let calling: HashMap<String, &Node> = (nodes.iter())
            .filter(|n| actions::owns(n) && n.actions.as_ref().is_some_and(|f| f.uses.is_some()))
            .map(|n| (n.identity(&out.repository), n))
            .collect();
        for consumer in &mut out.consumers {
            if !consumer
                .contract
                .as_deref()
                .is_some_and(|c| c.starts_with(PREFIX))
            {
                continue;
            }
            let call = consumer.via.as_deref().and_then(Call::read);
            consumer.git_ref = call.map(|c| c.git_ref.to_string());
            if let Some(facts) = calling.get(&consumer.site).and_then(|n| n.actions.as_ref()) {
                consumer.with = facts.with.clone();
                consumer.secrets = facts.secrets.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_names_owner_repository_pipeline_file_and_ref() {
        let call = Call::read("Acme/Workflows/.github/workflows/publish.yml@release/v1").unwrap();
        assert_eq!(
            (call.owner, call.repo, call.path, call.git_ref),
            (
                "Acme",
                "Workflows",
                ".github/workflows/publish.yml",
                "release/v1"
            )
        );
        for other in [
            // The forms of later tickets (a local call, an action) and what names nothing.
            "./.github/workflows/publish.yml",
            "acme/workflows/.github/workflows/publish.yml",
            "acme/workflows/.github/workflows/sub/publish.yml@main",
            "acme/workflows/publish.yml@main",
            "acme/workflows@main",
            "actions/checkout@v4",
            "docker://alpine:3",
            "acme/workflows/.github/workflows/publish.yml@",
            // A control character is in no name GitHub takes, and would break a line.
            "acme/workflows/.github/workflows/publish.yml@main\n  consumer: forged",
            "acme/workflows/.github/workflows/pub\tlish.yml@main",
        ] {
            assert!(Call::read(other).is_none(), "{other}");
        }
    }

    #[test]
    fn a_reason_flags_only_a_full_commit_id_other_than_the_indexed_commit() {
        let indexed = "0123456789abcdef0123456789abcdef01234567";
        let other = "f".repeat(40);
        for (git_ref, expected) in [
            ("main", "@main (indexed 0123456)"),
            (indexed, &format!("@{indexed} (indexed 0123456)")[..]),
            (
                &other[..],
                &format!("@{other} (indexed 0123456), ref mismatch")[..],
            ),
            // An abbreviated id reads as a name, and so does a branch of another length.
            ("0123456", "@0123456 (indexed 0123456)"),
            ("fffffff", "@fffffff (indexed 0123456)"),
        ] {
            assert_eq!(reason(git_ref, Some(indexed)), expected);
        }
        let long = "a".repeat(64);
        assert_eq!(
            reason(&long, Some(indexed)),
            format!("@{long} (indexed 0123456), ref mismatch")
        );
        assert_eq!(reason("main", None), "@main (indexed commit unknown)");
    }

    #[test]
    fn a_repository_without_origin_has_no_host() {
        assert_eq!(host("github.com/acme/app"), Some("github.com"));
        assert_eq!(host("local/app"), None);
    }
}
