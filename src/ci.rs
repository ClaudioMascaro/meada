//! The CI contract kind (ADR 0028, spec #820): `ci::<repository identity>[/<path>]` links a
//! reusable pipeline or an action to every CI job or CI step of the workspace that calls it.
//! Both sides sit on the IaC nodes the GitHub Actions reader stores (`iac::actions`): the
//! provider on the file node of a pipeline with `on.workflow_call` or of an action, the consumer
//! on the CI job or CI step whose `uses` names it. A `uses` writes no host, so the host is the
//! consumer's own, and one whose repository is not in the workspace holds no side at all. The
//! Git ref is an attribute of the consumer, never part of the identifier: it is shown as written
//! and never resolved. A CI step's `./<folder>` is read in the CI runner's workspace, so it is
//! unresolved wherever that is not known to be the file's repository (`workspace_causes`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use meada_core::export as written;
use meada_core::export::{Access, AllPermissions, Permissions};

use crate::contract::{ContractKind, Found, Hit, Level, Link, Role, Search};
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

const ABSENT: &str = " (absent at the indexed commit)";
const NOT_REUSABLE: &str =
    " (absent at the indexed commit: the file is no reusable pipeline there)";

/// The mark of a consumer of `contract` when no repository of the workspace provides it: its
/// path names no provider at the indexed commit, and may name one at the Git ref, so the mark
/// never says broken. A pipeline file that is there with no `on.workflow_call` says so.
fn absent(exports: &[Export], contract: &str) -> Option<&'static str> {
    let rest = contract.strip_prefix(PREFIX)?;
    // A provider's identifier starts with its own repository: no other export is read.
    let mut named = (exports.iter()).filter(|e| rest.starts_with(e.repository.as_str()));
    if (named.clone()).any(|e| e.providers.iter().any(|p| p.contract == contract)) {
        return None;
    }
    let plain = named.any(|e| {
        let path = (rest.strip_prefix(e.repository.as_str())).and_then(|at| at.strip_prefix('/'));
        // An action's file is no pipeline: a CI step that names it names a folder not there.
        let file = |n: &Node| {
            actions::owns(n)
                && n.address == "."
                && n.ty != actions::ACTION
                && Some(&*n.path) == path
        };
        e.iac.iter().any(file)
    });
    Some(if plain { NOT_REUSABLE } else { ABSENT })
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

/// A `permissions` block as an answer shows it: its word, or what it names (`{}` for none).
fn block(permissions: &Permissions) -> String {
    match permissions {
        Permissions::All(AllPermissions::ReadAll) => "read-all".into(),
        Permissions::All(AllPermissions::WriteAll) => "write-all".into(),
        Permissions::Named(named) if named.is_empty() => "{}".into(),
        Permissions::Named(named) => {
            let levels = named
                .iter()
                .map(|(name, level)| format!("{name}: {}", level.name()));
            levels.collect::<Vec<_>>().join(", ")
        }
    }
}

/// What the CI jobs of the pipeline `file` of `export` declare together: the highest level one
/// of them gives every permission none names, and each permission one names. None when no CI
/// job there holds a block: nothing is declared, and Meada guesses no default.
fn asked<'e>(export: &'e Export, file: &str) -> Option<(Access, BTreeMap<&'e str, Access>)> {
    let blocks: Vec<&Permissions> = (export.iac.iter())
        .filter(|n| actions::owns(n) && n.ty == actions::JOB && n.path == file)
        .filter_map(|n| n.actions.as_ref()?.permissions.as_ref())
        .collect();
    let rest = blocks.iter().map(|block| block.rest()).max()?;
    let names = blocks.iter().flat_map(|block| match block {
        Permissions::Named(named) => named.keys().collect(),
        Permissions::All(_) => Vec::new(),
    });
    let highest = |name: &str| blocks.iter().map(|block| block.level(name)).max();
    let named = names.map(|name| (name.as_str(), highest(name).unwrap_or(rest)));
    Some((rest, named.collect()))
}

/// The line of a reusable pipeline's `context` that says what its CI jobs declare together.
fn asked_line(export: &Export, file: &str) -> String {
    let Some((rest, named)) = asked(export, file) else {
        return "  permissions: not declared".into();
    };
    let word = match rest {
        Access::None => None,
        Access::Read => Some("read-all".to_string()),
        Access::Write => Some("write-all".to_string()),
    };
    let above = named.iter().filter(|(_, level)| **level != rest);
    let parts: Vec<String> = (word.into_iter())
        .chain(above.map(|(name, level)| format!("{name}: {}", level.name())))
        .collect();
    match parts.is_empty() {
        true => "  permissions: {}".into(),
        false => format!("  permissions: {}", parts.join(", ")),
    }
}

/// The permissions a CI job of the provider at the site `provider` declares above what the
/// block of the CI job at `consumer` holds, each with both levels. None when either side
/// declares nothing, and for a CI step, which calls an action: only a CI job is compared.
fn mismatch(exports: &[Export], consumer: &str, provider: &str) -> Vec<String> {
    let held = node_at(exports, consumer)
        .filter(|(_, node)| node.ty == actions::JOB)
        .and_then(|(_, node)| node.actions.as_ref()?.permissions.as_ref());
    let pipeline = node_at(exports, provider).and_then(|(repository, file)| {
        let export = exports.iter().find(|e| e.repository == repository)?;
        asked(export, &file.path)
    });
    let (Some(held), Some((rest, named))) = (held, pipeline) else {
        return Vec::new();
    };
    let own: Vec<&str> = match held {
        Permissions::Named(own) => own.keys().map(String::as_str).collect(),
        Permissions::All(_) => Vec::new(),
    };
    let names: BTreeSet<&str> = named.keys().copied().chain(own).collect();
    let level = |name: &str| named.get(name).copied().unwrap_or(rest);
    let against =
        |what: &str, a: Access, b: Access| format!("{what} {} against {}", a.name(), b.name());
    let mut above: Vec<String> = (names.into_iter())
        .filter(|name| level(name) > held.level(name))
        .map(|name| against(name, level(name), held.level(name)))
        .collect();
    if rest > held.rest() {
        above.push(against("every other permission", rest, held.rest()));
    }
    above
}

/// The keys a call passes and, for a CI job, the `permissions` block that governs it, as an
/// answer shows them after it: ` with: a, b; secrets: inherit; permissions: contents: read`.
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
    // Declared, never granted: a CI job with no block of its own or of its pipeline says so.
    if node.ty == actions::JOB {
        let declared = facts.permissions.as_ref().map(block);
        let declared = declared.unwrap_or_else(|| "not declared".into());
        parts.push(format!("permissions: {declared}"));
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
            for node in (export.iac.iter()).filter(|n| actions::owns(n) && n.address == ".") {
                // A folder named as a pipeline file would share that file's identifier.
                let path = match node.ty.as_str() {
                    actions::ACTION => actions::action_folder(&node.path)
                        .map(|folder| folder.trim_end_matches('/'))
                        .filter(|folder| !actions::is_workflow(folder)),
                    _ => (node.actions.as_ref().is_some_and(|f| f.reusable))
                        .then_some(node.path.as_str()),
                };
                if let Some(path) = path {
                    export.providers.push(Provider {
                        contract: contract(own, path),
                        site: node.identity(own),
                        ..Provider::default()
                    });
                }
            }
            crate::index::sort_providers(&mut export.providers);
        }
        // A folder with a `Dockerfile` and no action is a container action GitHub builds, which
        // no reader takes: a call to it holds no side, as one to a file outside the workspace.
        let provided: HashSet<&str> = (exports.iter().flat_map(|e| &e.providers))
            .map(|p| p.contract.as_str())
            .collect();
        let mut unread = HashSet::new();
        for export in exports.iter() {
            let folders = export.dockerfiles.iter();
            let contracts = folders.map(|folder| contract(&export.repository, folder));
            unread.extend(contracts.filter(|contract| !provided.contains(contract.as_str())));
        }
        for export in exports.iter_mut() {
            let own = export.repository.as_str();
            let causes = workspace_causes(own, &export.iac);
            for node in (export.iac.iter()).filter(|n| actions::owns(n) && n.address != ".") {
                let step = node.ty == actions::STEP;
                let Some(uses) = node.actions.as_ref().and_then(|f| f.uses.as_deref()) else {
                    continue;
                };
                let Some(contract) = consumed(uses, step, own, &known) else {
                    continue;
                };
                let site = node.identity(own);
                // Unresolved, the folder may be another repository's: the open item stays.
                let resolved = !causes.contains_key(&site);
                if resolved && unread.contains(&contract) {
                    continue;
                }
                export.consumers.push(Consumer {
                    // Unresolved: no contract, as a call whose URL could not be read.
                    contract: resolved.then_some(contract),
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
            crate::index::sort_consumers(&mut export.consumers);
        }
    }
    /// The mark of a consumer no provider answers (`absent`). A provider nothing calls carries
    /// none: `meada status` lists it, under the scope rule.
    fn side_note(&self, exports: &[Export], contract: &str, role: Role) -> &'static str {
        match role {
            Role::Consumer => absent(exports, contract).unwrap_or_default(),
            _ => "",
        }
    }
    /// The providers of `export` that nothing calls, only when the scope holds a provider and
    /// a consumer of the kind (ADR 0026, "Orphans"). An unresolved consumer holds no contract
    /// and a call outside the workspace no side: neither opens the scope.
    fn scope_status(
        &self,
        _: &Repository,
        export: &Export,
        exports: &[Export],
        _: &[Link],
    ) -> Vec<String> {
        let consumed: HashSet<&str> = (exports.iter().flat_map(|e| &e.consumers))
            .filter_map(|c| c.contract.as_deref())
            .filter(|contract| contract.starts_with(PREFIX))
            .collect();
        if consumed.is_empty() {
            return Vec::new();
        }
        let ours = export
            .providers
            .iter()
            .filter(|p| p.contract.starts_with(PREFIX));
        ours.filter(|p| !consumed.contains(p.contract.as_str()))
            .map(|p| {
                let place = self.place(&export.repository, &p.site).unwrap_or_default();
                let (contract, place) = (p.contract.as_str(), place.as_str());
                crate::msg::Msg::OrphanCi { contract, place }.text()
            })
            .collect()
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
                let mut reason = reason(uses.git_ref(), indexed);
                let above = mismatch(exports, &consumer.site, &provider.site);
                if !above.is_empty() {
                    reason.push_str(&format!(", permissions mismatch: {}", above.join(", ")));
                }
                Link::new(
                    consumer.site.clone(),
                    provider.site.clone(),
                    contract.into(),
                    Level::Exact,
                    reason,
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
                // What its CI jobs declare together, which each consumer's block is held against.
                let file = node_at(exports, site).filter(|(_, n)| n.ty != actions::ACTION);
                lines.extend(file.map(|(_, file)| asked_line(export, &file.path)));
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
                let mark = absent(exports, contract).unwrap_or_default();
                lines.push(format!("consumes {contract}{git_ref}{keys}{mark}"));
                let called =
                    (links.iter()).filter(|l| l.consumer == site && l.contract == contract);
                lines.extend(called.filter_map(|l| side("provider", &l.provider, l)));
            }
        }
        lines
    }
    /// A call's Git ref and the keys it passes, on its consumer row, and the absent mark where
    /// the rows of `out` hold no link for it: a link is written for every provider of its
    /// contract, so none means the workspace holds none.
    fn written(&self, nodes: &[Node], out: &mut written::Export) {
        let linked: HashSet<(&str, &str)> = (out.links.iter())
            .map(|l| (l.consumer.as_str(), l.contract.as_str()))
            .collect();
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
            let contract = consumer.contract.as_deref().unwrap_or_default();
            consumer.absent = !linked.contains(&(consumer.site.as_str(), contract));
            if let Some(facts) = calling.get(&consumer.site).and_then(|n| n.actions.as_ref()) {
                consumer.with = facts.with.clone();
                consumer.secrets = facts.secrets.clone();
                consumer.permissions = facts.permissions.clone();
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
