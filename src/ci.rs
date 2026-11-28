use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use meada_core::export as written;
use meada_core::export::{Access, AllPermissions, Permissions};

use crate::contract::{ContractKind, Found, Hit, Level, Link, Role, Search};
use crate::iac::{Node, actions};
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::msg;
use crate::resolve::Code;
use crate::workspace::Repository;

pub struct Ci;

const PREFIX: &str = "ci::";

enum Uses<'a> {
    Peer {
        owner: &'a str,
        repo: &'a str,
        path: &'a str,
        git_ref: &'a str,
    },
    Rooted(&'a str),
    Local(&'a str),
}

impl<'a> Uses<'a> {
    fn read(uses: &'a str) -> Option<Self> {
        let named = !uses.contains(char::is_control) && !uses.contains("${{");
        if !named || uses.starts_with("docker://") {
            return None;
        }
        if let Some(path) = uses.strip_prefix("$/") {
            return (!path.contains('@')).then_some(Self::Rooted(path));
        }
        if let Some(path) = uses.strip_prefix("./") {
            return Some(Self::Local(path));
        }
        let (target, git_ref) = uses.split_once('@')?;
        let mut parts = target.splitn(3, '/');
        let (owner, repo) = (parts.next()?, parts.next()?);
        let named =
            ![owner, repo, git_ref].iter().any(|part| part.is_empty()) && !git_ref.contains(' ');
        named.then_some(Self::Peer {
            owner,
            repo,
            path: parts.next().unwrap_or_default(),
            git_ref,
        })
    }

    fn git_ref(&self) -> Option<&'a str> {
        match self {
            Self::Peer { git_ref, .. } => Some(git_ref),
            Self::Rooted(_) | Self::Local(_) => None,
        }
    }
}

fn contract(repository: &str, path: &str) -> String {
    match path {
        "" => format!("{PREFIX}{repository}"),
        path => format!("{PREFIX}{repository}/{path}"),
    }
}

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

fn workspace_causes(own: &str, nodes: &[Node]) -> HashMap<String, &'static str> {
    let place = |n: &'_ Node| {
        let (parent, index) = n.address.strip_suffix(']')?.rsplit_once(".steps[")?;
        (n.ty == actions::STEP).then_some((parent.len(), index.parse::<usize>().ok()?))
    };
    let checkout = |uses: &str| {
        let target = uses.split_once('@').map(|(target, _)| target);
        target.is_some_and(|target| target.eq_ignore_ascii_case("actions/checkout"))
    };
    let mut callers: HashMap<&str, &'static str> = HashMap::new();
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

fn absent(exports: &[Export], contract: &str) -> Option<&'static str> {
    let rest = contract.strip_prefix(PREFIX)?;
    let mut named = (exports.iter()).filter(|e| rest.starts_with(e.repository.as_str()));
    if (named.clone()).any(|e| e.providers.iter().any(|p| p.contract == contract)) {
        return None;
    }
    let plain = named.any(|e| {
        let path = (rest.strip_prefix(e.repository.as_str())).and_then(|at| at.strip_prefix('/'));
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

fn host(identity: &str) -> Option<&str> {
    let (host, _) = identity.split_once('/')?;
    (host != "local").then_some(host)
}

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

type Asked<'e> = (Access, BTreeMap<&'e str, Access>);

fn governed(export: &Export) -> impl Iterator<Item = (&Node, &Permissions)> {
    let jobs = (export.iac.iter()).filter(|n| actions::owns(n) && n.ty == actions::JOB);
    jobs.filter_map(|n| Some((n, n.actions.as_ref()?.permissions.as_ref()?)))
}

fn asked<'e>(blocks: impl Iterator<Item = &'e Permissions>) -> Option<Asked<'e>> {
    let mut rest = None;
    let mut named: BTreeMap<&str, Access> = BTreeMap::new();
    for block in blocks {
        rest = rest.max(Some(block.rest()));
        if let Permissions::Named(levels) = block {
            for (name, &level) in levels {
                let highest = named.entry(name.as_str()).or_insert(level);
                *highest = level.max(*highest);
            }
        }
    }
    let rest = rest?;
    named
        .values_mut()
        .for_each(|level| *level = rest.max(*level));
    Some((rest, named))
}

fn asked_line(export: &Export, file: &str) -> String {
    let blocks = governed(export).filter(|(job, _)| job.path == file);
    let Some((rest, named)) = asked(blocks.map(|(_, block)| block)) else {
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

fn mismatch(held: &Permissions, asked: &Asked<'_>) -> Vec<String> {
    let (rest, named) = (asked.0, &asked.1);
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

fn sites(exports: &[Export]) -> HashMap<String, (&Export, &Node)> {
    let mut sites = HashMap::new();
    for export in exports {
        for node in export.iac.iter().filter(|n| actions::owns(n)) {
            let site = node.identity(&export.repository);
            sites.entry(site).or_insert((export, node));
        }
    }
    sites
}

fn links<'a>(
    exports: &'a [Export],
    providers: &[(&'a Repository, &'a Provider)],
    consumers: impl Iterator<Item = &'a Consumer>,
) -> Vec<Link> {
    let mut serving: HashMap<&str, Vec<(&Repository, &Provider)>> = HashMap::new();
    for &(repository, provider) in providers {
        if provider.contract.starts_with(PREFIX) {
            let serving = serving.entry(provider.contract.as_str()).or_default();
            serving.push((repository, provider));
        }
    }
    let commits: HashMap<&str, Option<&str>> = (exports.iter().rev())
        .map(|e| (e.repository.as_str(), e.commit.as_deref()))
        .collect();
    let nodes = OnceCell::new();
    let node = |site: &str| nodes.get_or_init(|| sites(exports)).get(site).copied();
    let declared = OnceCell::new();
    let blocks = || {
        let mut blocks: HashMap<(&str, &str), Vec<&Permissions>> = HashMap::new();
        for export in exports {
            for (job, block) in governed(export) {
                let file = (export.repository.as_str(), job.path.as_str());
                blocks.entry(file).or_default().push(block);
            }
        }
        blocks
    };
    let mut asked_at: HashMap<&str, Option<Asked<'_>>> = HashMap::new();
    let mut links = Vec::new();
    for consumer in consumers {
        let Some(contract) = consumer.contract.as_deref() else {
            continue;
        };
        let (Some(serving), Some(uses)) = (
            serving.get(contract),
            consumer.via.as_deref().and_then(Uses::read),
        ) else {
            continue;
        };
        let held = node(&consumer.site)
            .filter(|(_, node)| node.ty == actions::JOB)
            .and_then(|(_, node)| node.actions.as_ref()?.permissions.as_ref());
        for &(repository, provider) in serving {
            let indexed = commits.get(repository.identity.as_str()).copied().flatten();
            let mut reason = reason(uses.git_ref(), indexed);
            let above = held.map_or(Vec::new(), |held| {
                let asked = asked_at.entry(provider.site.as_str()).or_insert_with(|| {
                    let (export, file) = node(&provider.site)?;
                    let file = (export.repository.as_str(), file.path.as_str());
                    asked(declared.get_or_init(blocks).get(&file)?.iter().copied())
                });
                asked
                    .as_ref()
                    .map_or(Vec::new(), |asked| mismatch(held, asked))
            });
            if !above.is_empty() {
                reason.push_str(&format!(", permissions mismatch: {}", above.join(", ")));
            }
            links.push(Link::new(
                consumer.site.clone(),
                provider.site.clone(),
                contract.into(),
                Level::Exact,
                reason,
                None,
                None,
                None,
            ));
        }
    }
    links
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
    fn own_search(&self, exports: &[Export], _: &[Link], needle: &str) -> Vec<Hit> {
        (exports.iter())
            .flat_map(|export| actions::hits(&export.repository, &export.iac, needle))
            .collect()
    }
    fn in_default_search(&self) -> bool {
        false
    }
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
    fn calls_from_definition(&self) -> bool {
        true
    }
    fn starts_at_provider(&self) -> bool {
        true
    }
    fn workspace_step(&self, exports: &mut [Export], repositories: &[Repository]) {
        let known: HashSet<&str> = repositories.iter().map(|r| r.identity.as_str()).collect();
        for export in exports.iter_mut() {
            let own = export.repository.as_str();
            for node in (export.iac.iter()).filter(|n| actions::owns(n) && n.address == ".") {
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
                let resolved = !causes.contains_key(&site);
                if resolved && unread.contains(&contract) {
                    continue;
                }
                export.consumers.push(Consumer {
                    contract: resolved.then_some(contract),
                    site,
                    via: Some(uses.to_string()),
                    shape: None,
                    method: None,
                    identity: None,
                    fragments: Vec::new(),
                    hops: Vec::new(),
                    root: None,
                    lead: None,
                    cap: None,
                    client: None,
                });
            }
            crate::index::sort_consumers(&mut export.consumers);
        }
    }
    fn side_note(&self, exports: &[Export], contract: &str, role: Role) -> &'static str {
        match role {
            Role::Consumer => absent(exports, contract).unwrap_or_default(),
            _ => "",
        }
    }
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
        let scope = msg::Scope::of(exports);
        let ours = export
            .providers
            .iter()
            .filter(|p| p.contract.starts_with(PREFIX));
        ours.filter(|p| !consumed.contains(p.contract.as_str()))
            .map(|p| {
                let place = self.place(&export.repository, &p.site).unwrap_or_default();
                let (contract, place) = (p.contract.as_str(), place.as_str());
                msg::Msg::Orphan {
                    contract,
                    absent: msg::Absent::Consumer,
                    scope,
                    place,
                }
                .text()
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
        links(exports, providers, std::iter::once(consumer))
    }
    fn workspace_links(
        &self,
        exports: &[Export],
        providers: &[(&Repository, &Provider)],
    ) -> Option<Vec<Link>> {
        let consumers = exports.iter().flat_map(|e| &e.consumers);
        Some(links(exports, providers, consumers))
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
        let nodes = OnceCell::new();
        let node = |site: &str| nodes.get_or_init(|| sites(exports)).get(site).copied();
        let side = |word: &str, other: &str, link: &Link| {
            let (export, node) = node(other)?;
            let keys = if word == "consumer" {
                passed(node)
            } else {
                String::new()
            };
            Some(format!(
                "  {word}: {} [{}: {}]{keys}",
                crate::iac::at(&export.repository, node),
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
                let file = node(site).filter(|(_, n)| n.ty != actions::ACTION);
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
                let keys = node(site).map_or(String::new(), |(_, n)| passed(n));
                let mark = absent(exports, contract).unwrap_or_default();
                lines.push(format!("consumes {contract}{git_ref}{keys}{mark}"));
                let called =
                    (links.iter()).filter(|l| l.consumer == site && l.contract == contract);
                lines.extend(called.filter_map(|l| side("provider", &l.provider, l)));
            }
        }
        lines
    }
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

    fn consumed_by(uses: &str, step: bool) -> Option<String> {
        let known = HashSet::from(["github.com/acme/app", "github.com/acme/workflows"]);
        consumed(uses, step, "github.com/acme/app", &known)
    }

    #[test]
    fn a_ci_job_consumes_a_pipeline_file_and_nothing_else() {
        let publish = "ci::github.com/acme/workflows/.github/workflows/publish.yml";
        for (uses, expected) in [
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
            ("other/pipelines/.github/workflows/deploy.yml@v3", None),
            (
                "acme/workflows/.github/workflows/publish.yml@main\n  consumer: forged",
                None,
            ),
            ("acme/workflows/.github/workflows/pub\tlish.yml@main", None),
            (
                "acme/workflows/.github/workflows/publish.yml@main (indexed 0000000), ref mismatch",
                None,
            ),
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
            ("./", Some("ci::github.com/acme/app")),
            ("$/", Some("ci::github.com/acme/app")),
            ("./../outside", None),
            ("$/tools/../../outside", None),
            ("acme/workflows/../other@v1", None),
            ("docker://alpine:3", None),
            ("docker://ghcr.io/acme/workflows@sha256:0123", None),
            ("acme/workflows/.github/workflows/publish.yml@main", None),
            ("./.github/workflows/ci.yml", None),
            ("actions/checkout@v4", None),
            ("acme/workflows", None),
            ("acme/workflows/tools/setup@", None),
            ("$/tools/local@v1", None),
            ("acme/workflows@v1\n  consumer: forged", None),
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
        assert_eq!(reason(None, Some(indexed)), "own repository");
    }

    #[test]
    fn a_repository_without_origin_has_no_host() {
        assert_eq!(host("github.com/acme/app"), Some("github.com"));
        assert_eq!(host("local/app"), None);
    }
}
