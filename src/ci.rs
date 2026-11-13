//! The CI contract kind (ADR 0028, spec #820): `ci::<repository identity>[/<path>]` links a
//! reusable pipeline or an action to every CI job or CI step of the workspace that calls it.
//! Both sides sit on the IaC nodes the GitHub Actions reader stores (`iac::actions`): the
//! provider on the file node of a pipeline with `on.workflow_call` or of an action, the consumer
//! on the CI job or CI step whose `uses` names it. A `uses` writes no host, so the host is the
//! consumer's own, and one whose repository is not in the workspace holds no side at all. The
//! Git ref is an attribute of the consumer, never part of the identifier: it is shown as written
//! and never resolved. A CI step's `./<folder>` is read in the CI runner's workspace, so it is
//! unresolved wherever that is not known to be the file's repository (`workspace_causes`).

use std::collections::{HashMap, HashSet};

use meada_core::export as written;

use crate::contract::{ContractKind, Found, Hit, Level, Link, Search};
use crate::iac::{Node, actions};
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::resolve::Code;
use crate::workspace::Repository;

pub struct Ci;

const PREFIX: &str = "ci::";

/// What a `uses` names, in the forms GitHub reads (ADR 0028). It holds no expression, so it is
/// literal. `docker://<image>` names an image, no file of a repository: it reads as none.
enum Uses<'a> {
    /// `<owner>/<repo>[/<path>]@<ref>`: a file or a folder of a repository, at a Git ref.
    Peer {
        owner: &'a str,
        repo: &'a str,
        path: &'a str,
        git_ref: &'a str,
    },
    /// `$/<path>`: of the repository of the file that writes it.
    Rooted(&'a str),
    /// `./<path>`: of the own repository under a CI job, of the CI runner's workspace under a
    /// CI step.
    Local(&'a str),
}

impl<'a> Uses<'a> {
    fn read(uses: &'a str) -> Option<Self> {
        // No name GitHub takes holds a control character, and one would break an answer's line.
        // It evaluates no expression here either: a `uses` that holds one names nothing.
        let named = !uses.contains(char::is_control) && !uses.contains("${{");
        if !named || uses.starts_with("docker://") {
            return None;
        }
        if let Some(path) = uses.strip_prefix("$/") {
            return Some(Self::Rooted(path));
        }
        if let Some(path) = uses.strip_prefix("./") {
            return Some(Self::Local(path));
        }
        let (target, git_ref) = uses.split_once('@')?;
        let mut parts = target.splitn(3, '/');
        let (owner, repo) = (parts.next()?, parts.next()?);
        let named = ![owner, repo, git_ref].iter().any(|part| part.is_empty());
        named.then_some(Self::Peer {
            owner,
            repo,
            path: parts.next().unwrap_or_default(),
            git_ref,
        })
    }

    /// The Git ref the call names its provider at: none for a call inside its repository.
    fn git_ref(&self) -> Option<&'a str> {
        match self {
            Self::Peer { git_ref, .. } => Some(git_ref),
            Self::Rooted(_) | Self::Local(_) => None,
        }
    }
}

/// The identifier of what sits at `path` of `repository`: a pipeline file, the folder of an
/// action, or nothing for an action at the root.
fn contract(repository: &str, path: &str) -> String {
    match path {
        "" => format!("{PREFIX}{repository}"),
        path => format!("{PREFIX}{repository}/{path}"),
    }
}

/// The contract the call `uses` consumes, written in the repository `own` by a CI step or by a
/// CI job. None for a repository outside the workspace (`known`), for a path that leaves the
/// root, and where GitHub runs nothing: a pipeline file under a CI step, anything else under a
/// CI job.
fn consumed(uses: &str, step: bool, own: &str, known: &HashSet<&str>) -> Option<String> {
    let (repository, path) = match Uses::read(uses)? {
        Uses::Peer {
            owner, repo, path, ..
        } => {
            let (owner, repo) = (owner.to_lowercase(), repo.to_lowercase());
            let repository = format!("{}/{owner}/{repo}", host(own)?);
            (
                known.contains(repository.as_str()).then_some(repository)?,
                path,
            )
        }
        Uses::Rooted(path) | Uses::Local(path) => (own.to_string(), path),
    };
    let path = crate::iac::joined("", path)?;
    (actions::is_workflow(&path) != step).then(|| contract(&repository, &path))
}

const COMPOSITE: &str = "a composite action runs in the workspace of its caller";
const REUSABLE: &str = "a checkout in a reusable pipeline is of its caller's repository";
const NO_CHECKOUT: &str = "no checkout before the CI step";
const ELSEWHERE: &str = "a checkout before the CI step names `repository` or `path`";

/// Why a CI step's `./<folder>` is unresolved, for each such CI step of `nodes` by its site.
/// The CI runner reads the folder in its workspace, which is known to be the repository of the
/// file only in a pipeline that is not reusable, once its CI job checked that repository out
/// at the default path: an `actions/checkout` before the CI step, and none before it that
/// names `repository` or `path`.
fn workspace_causes(own: &str, nodes: &[Node]) -> HashMap<String, &'static str> {
    // Where a CI step sits: the address of what holds it, and its index there.
    let place = |n: &'_ Node| {
        let (parent, index) = n.address.strip_suffix(']')?.rsplit_once(".steps[")?;
        (n.ty == actions::STEP).then_some((parent.len(), index.parse::<usize>().ok()?))
    };
    let checkout = |uses: &str| {
        let target = uses.split_once('@').map(|(target, _)| target);
        target.is_some_and(|target| target.eq_ignore_ascii_case("actions/checkout"))
    };
    // A file whose CI steps run in the workspace of another: its cause holds for each.
    let mut callers: HashMap<&str, &'static str> = HashMap::new();
    // By file and CI job: the first checkout at the default path, and the first elsewhere.
    let mut checkouts: HashMap<(&str, &str), [Option<usize>; 2]> = HashMap::new();
    let mut local = Vec::new();
    for n in nodes.iter().filter(|n| actions::owns(n)) {
        let facts = n.actions.as_ref();
        if n.address == "." {
            match (n.ty.as_str(), facts.is_some_and(|f| f.reusable)) {
                (actions::ACTION, _) => _ = callers.insert(&n.path, COMPOSITE),
                (_, true) => _ = callers.insert(&n.path, REUSABLE),
                _ => {}
            }
            continue;
        }
        let (Some((parent, index)), Some(facts)) = (place(n), facts) else {
            continue;
        };
        let Some(uses) = facts.uses.as_deref() else {
            continue;
        };
        let job = &n.address[..parent];
        if uses.starts_with("./") {
            local.push((n, job, index));
        } else if checkout(uses) {
            // GitHub reads an input's key in any case (`INPUT_REPOSITORY`).
            let names = |key: &String, input| key.eq_ignore_ascii_case(input);
            let elsewhere =
                (facts.with.iter()).any(|key| names(key, "repository") || names(key, "path"));
            let first = &mut checkouts.entry((&n.path, job)).or_default()[usize::from(elsewhere)];
            *first = Some(first.map_or(index, |at| at.min(index)));
        }
    }
    let mut causes = HashMap::new();
    for (n, job, index) in local {
        let before = |first: Option<usize>| first.is_some_and(|at| at < index);
        let [own_first, elsewhere] = (checkouts.get(&(n.path.as_str(), job)))
            .copied()
            .unwrap_or_default();
        let cause = match callers.get(n.path.as_str()) {
            Some(cause) => cause,
            None if before(elsewhere) => ELSEWHERE,
            None if before(own_first) => continue,
            None => NO_CHECKOUT,
        };
        causes.insert(n.identity(own), cause);
    }
    causes
}

/// The host of a repository identity, which a `uses` leaves out: none for a repository without
/// `origin`, which names no peer.
fn host(identity: &str) -> Option<&str> {
    let (host, _) = identity.split_once('/')?;
    (host != "local").then_some(host)
}

/// A link's reason: the Git ref as written and the provider's indexed commit, with `ref
/// mismatch` only when the ref is a full commit id (40 or 64 hexadecimal digits) other than it.
/// A call inside its repository writes no ref, and its reason holds none.
fn reason(git_ref: Option<&str>, indexed: Option<&str>) -> String {
    let Some(git_ref) = git_ref else {
        return "own repository".into();
    };
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
            let causes = workspace_causes(own, &export.iac);
            for node in export.iac.iter().filter(|n| actions::owns(n)) {
                let facts = node.actions.as_ref();
                if node.address == "." {
                    // A folder named as a pipeline file would share that file's identifier.
                    let path = match node.ty.as_str() {
                        actions::ACTION => actions::action_folder(&node.path)
                            .map(|folder| folder.trim_end_matches('/'))
                            .filter(|folder| !actions::is_workflow(folder)),
                        _ => (facts.is_some_and(|f| f.reusable)).then_some(node.path.as_str()),
                    };
                    if let Some(path) = path {
                        export.providers.push(Provider {
                            contract: contract(own, path),
                            site: node.identity(own),
                            ..Provider::default()
                        });
                    }
                    continue;
                }
                let step = node.ty == actions::STEP;
                let Some(uses) = facts.and_then(|f| f.uses.as_deref()) else {
                    continue;
                };
                let Some(contract) = consumed(uses, step, own, &known) else {
                    continue;
                };
                let site = node.identity(own);
                export.consumers.push(Consumer {
                    // Unresolved: no contract, as a call whose URL could not be read.
                    contract: (!causes.contains_key(&site)).then_some(contract),
                    site,
                    via: Some(uses.to_string()),
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
    fn unresolved_causes(&self, exports: &[Export]) -> HashMap<String, &'static str> {
        let mut causes = HashMap::new();
        for export in exports {
            let found = workspace_causes(&export.repository, &export.iac);
            let open = export.consumers.iter().filter(|c| c.contract.is_none());
            causes.extend(open.filter_map(|c| Some((c.site.clone(), *found.get(&c.site)?))));
        }
        causes
    }
    fn link(
        &self,
        consumer: &Consumer,
        _: &Repository,
        providers: &[(&Repository, &Provider)],
        exports: &[Export],
    ) -> Vec<Link> {
        let (Some(contract), Some(uses)) = (
            consumer.contract.as_deref(),
            consumer.via.as_deref().and_then(Uses::read),
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
                    reason(uses.git_ref(), indexed),
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
                let uses = consumer.via.as_deref();
                let Some(contract) = consumer.contract.as_deref() else {
                    let causes = workspace_causes(&export.repository, &export.iac);
                    if let (Some(cause), Some(uses)) = (causes.get(site), uses) {
                        lines.push(format!("consumes (unresolved) {uses}: {cause}"));
                    }
                    continue;
                };
                if !ours(contract) {
                    continue;
                }
                let git_ref = uses.and_then(Uses::read).and_then(|u| u.git_ref());
                let git_ref = git_ref.map_or(String::new(), |r| format!(" @{r}"));
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
            let uses = consumer.via.as_deref().and_then(Uses::read);
            consumer.git_ref = uses.and_then(|u| u.git_ref()).map(str::to_string);
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

    /// The contract of `uses` in `github.com/acme/app`, beside `github.com/acme/workflows`.
    fn consumed_by(uses: &str, step: bool) -> Option<String> {
        let known = HashSet::from(["github.com/acme/app", "github.com/acme/workflows"]);
        consumed(uses, step, "github.com/acme/app", &known)
    }

    #[test]
    fn a_ci_job_consumes_a_pipeline_file_and_nothing_else() {
        let publish = "ci::github.com/acme/workflows/.github/workflows/publish.yml";
        for (uses, expected) in [
            // Owner and repository are lower-cased as the identity is; the path keeps its case.
            (
                "Acme/Workflows/.github/workflows/publish.yml@release/v1",
                Some(publish),
            ),
            (
                "./.github/workflows/ci.yml",
                Some("ci::github.com/acme/app/.github/workflows/ci.yml"),
            ),
            (
                "$/.github/workflows/ci.yml",
                Some("ci::github.com/acme/app/.github/workflows/ci.yml"),
            ),
            // What names no pipeline file: a folder, a file below the folder, no ref, an action.
            ("acme/workflows/.github/workflows/publish.yml", None),
            (
                "acme/workflows/.github/workflows/sub/publish.yml@main",
                None,
            ),
            ("acme/workflows/publish.yml@main", None),
            ("acme/workflows@main", None),
            ("acme/workflows/.github/workflows/publish.yml@", None),
            ("./tools/setup", None),
            ("$/tools/setup", None),
            ("./.github/workflows/ci.yml@main", None),
            ("docker://alpine:3", None),
            // A repository outside the workspace.
            ("other/pipelines/.github/workflows/deploy.yml@v3", None),
            // A control character is in no name GitHub takes, and would break a line.
            (
                "acme/workflows/.github/workflows/publish.yml@main\n  consumer: forged",
                None,
            ),
            ("acme/workflows/.github/workflows/pub\tlish.yml@main", None),
        ] {
            assert_eq!(consumed_by(uses, false).as_deref(), expected, "{uses}");
        }
    }

    #[test]
    fn a_ci_step_consumes_the_folder_of_an_action_and_nothing_else() {
        let setup = Some("ci::github.com/acme/workflows/tools/setup");
        let own = Some("ci::github.com/acme/app/tools/local");
        for (uses, expected) in [
            ("acme/workflows@v1", Some("ci::github.com/acme/workflows")),
            ("Acme/Workflows/tools/setup@v1", setup),
            ("acme/workflows/tools/./setup/@v1", setup),
            ("./tools/local", own),
            ("$/tools/local", own),
            ("./tools/../tools/local/", own),
            // The root of the repository holds an action too.
            ("./", Some("ci::github.com/acme/app")),
            ("$/", Some("ci::github.com/acme/app")),
            // A path that leaves the root, by the rule of `iac::joined`.
            ("./../outside", None),
            ("$/tools/../../outside", None),
            ("acme/workflows/../other@v1", None),
            // An image, a pipeline file, a repository outside the workspace, no ref.
            ("docker://alpine:3", None),
            ("docker://ghcr.io/acme/workflows@sha256:0123", None),
            ("acme/workflows/.github/workflows/publish.yml@main", None),
            ("./.github/workflows/ci.yml", None),
            ("actions/checkout@v4", None),
            ("acme/workflows", None),
            ("acme/workflows/tools/setup@", None),
            ("acme/workflows@v1\n  consumer: forged", None),
            // An expression, which GitHub refuses in a `uses`.
            ("./tools/${{ matrix.dir }}", None),
            ("acme/workflows/tools/setup@${{ inputs.ref }}", None),
        ] {
            assert_eq!(consumed_by(uses, true).as_deref(), expected, "{uses}");
        }
    }

    #[test]
    fn a_call_by_owner_and_name_needs_the_host_of_its_own_repository() {
        let known = HashSet::from(["local/app", "github.com/acme/workflows"]);
        let read = |uses: &str| consumed(uses, true, "local/app", &known);
        assert_eq!(read("acme/workflows/tools/setup@v1"), None);
        assert_eq!(
            read("./tools/local").as_deref(),
            Some("ci::local/app/tools/local")
        );
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
            assert_eq!(reason(Some(git_ref), Some(indexed)), expected);
        }
        let long = "a".repeat(64);
        assert_eq!(
            reason(Some(&long), Some(indexed)),
            format!("@{long} (indexed 0123456), ref mismatch")
        );
        assert_eq!(reason(Some("main"), None), "@main (indexed commit unknown)");
        // A call inside its repository: no ref to show, whatever the commit.
        assert_eq!(reason(None, Some(indexed)), "own repository");
    }

    #[test]
    fn a_repository_without_origin_has_no_host() {
        assert_eq!(host("github.com/acme/app"), Some("github.com"));
        assert_eq!(host("local/app"), None);
    }
}
