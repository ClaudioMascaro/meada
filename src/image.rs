//! Images connect workload runners to the repositories whose configuration builds them.

use crate::contract::Level;
use crate::contract::{ContractKind, Found, Link, Role};
use crate::iac::actions::run::{Part, Word, literal};
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::resolve::Code;
use crate::workspace::Repository;
use serde_json::Value;

pub(crate) mod code;
mod config;
mod dockerfile;
pub(crate) mod environment;
mod jib;
mod patterns;

pub(crate) use config::named_file;

pub struct Image;

/// Effective image links for execution, without replacing the raw links used to validate
/// decisions and list open items. Every consumer of workload scope shares this projection.
pub(crate) fn execution_links(exports: &[Export], repositories: &[Repository]) -> Vec<Link> {
    let providers = exports
        .iter()
        .zip(repositories)
        .flat_map(|(e, r)| {
            e.providers
                .iter()
                .filter(|p| p.contract.starts_with("image::"))
                .map(move |p| (r, p))
        })
        .collect::<Vec<_>>();
    let links = exports
        .iter()
        .zip(repositories)
        .flat_map(|(e, r)| {
            e.consumers
                .iter()
                .filter(|c| {
                    c.contract
                        .as_deref()
                        .is_some_and(|c| c.starts_with("image::"))
                })
                .flat_map(|c| Image.link(c, r, &providers, exports))
        })
        .collect();
    // Missing-target decisions cannot change these visible links. Their waiting/stale
    // reports are still computed with the real workspace by the ordinary Engine replay.
    crate::decisions::replay_repositories(links, repositories, &[], exports, |d| {
        d.target
            .split_once("::")
            .is_some_and(|(_, c)| c.starts_with("image::"))
            && !patterns::handles(d)
    })
    .0
}

impl ContractKind for Image {
    fn prefix(&self) -> &'static str {
        "image"
    }
    fn extract(&self, _: &Code, _: &Indexed) -> Vec<Found> {
        Vec::new()
    }
    fn config_file(&self, path: &str) -> bool {
        config::is_file(path)
    }
    fn config_summary(
        &self,
        path: &str,
        src: &[u8],
        root: &std::path::Path,
        facts: &crate::contract::ConfigFacts<'_>,
    ) -> Value {
        serde_json::to_value(config::read(path, src, root, facts)).unwrap_or(Value::Null)
    }
    fn sides(&self) -> Option<(&'static str, &'static str)> {
        Some(("builder", "runner"))
    }
    fn calls_from_definition(&self) -> bool {
        true
    }
    fn exports_repository_links(&self) -> bool {
        true
    }
    fn declare(&self, repo: &Repository, export: &mut Export) {
        for (path, summary) in summaries(export) {
            for side in summary.builders {
                export.providers.push(Provider {
                    contract: format!("image::{}", side.image),
                    site: step(export, &side.at).unwrap_or_else(|| repo.identity.clone()),
                    at: Some(side.at.clone()),
                    evidence: side.inferred.then_some(side.at),
                    ..Provider::default()
                });
            }
            for side in summary.runners {
                export.consumers.push(runner(
                    &repo.identity,
                    &path,
                    &side.key,
                    &side.image,
                    if crate::config_values::parent(&path).is_empty() {
                        "."
                    } else {
                        crate::config_values::parent(&path)
                    },
                    Some(format!("image: {path}#{} (Helm value)", side.key)),
                ));
                export.iac.push(crate::iac::Node {
                    address: side.key.clone(),
                    root: if crate::config_values::parent(&path).is_empty() {
                        ".".into()
                    } else {
                        crate::config_values::parent(&path).into()
                    },
                    ty: "HelmValues".into(),
                    path: path.clone(),
                    line: side.line,
                    end_line: side.line,
                    block: "image".into(),
                    ..Default::default()
                });
            }
        }
        // The builder rows of the command table (ADR 0028): the CI step is the site.
        for script in &export.scripts {
            let step = script.site.strip_prefix(&repo.identity);
            let Some((path, step)) = step.and_then(|s| s.strip_prefix("::")?.split_once("::"))
            else {
                continue;
            };
            let at = format!("{path}#{step}.run");
            // An image the script pushes under two tags is one side.
            let images: std::collections::BTreeSet<String> =
                script.commands.iter().flat_map(|c| pushed(c)).collect();
            for image in images {
                export.providers.push(Provider {
                    evidence: image.contains('*').then(|| at.clone()),
                    contract: format!("image::{image}"),
                    site: script.site.clone(),
                    at: Some(at.clone()),
                    ..Provider::default()
                });
            }
        }
        let runners = images(export)
            .into_iter()
            .filter_map(|(node, image, via)| {
                let name = evaluated(image)?;
                Some(runner(
                    &repo.identity,
                    &node.path,
                    &node.address,
                    &name,
                    &node.root,
                    via,
                ))
            })
            .collect::<Vec<_>>();
        export.consumers.extend(runners);
        for node in &export.iac {
            if node.ty != "service" || crate::iac::types::workload("compose", &node.ty).is_none() {
                continue;
            }
            let Some(image) = &node.image else {
                continue;
            };
            let Some(name) = evaluated(&image.text) else {
                continue;
            };
            let at = format!("{}#{}.image", node.path, node.block);
            let evidence = image
                .overridable
                .then(|| format!("image: {at} (Compose default)"));
            if node.build_image {
                export.providers.push(Provider {
                    contract: format!("image::{name}"),
                    site: repo.identity.clone(),
                    at: Some(at.clone()),
                    evidence: evidence.map(|_| at),
                    ..Provider::default()
                });
            }
        }
        for (path, summary) in summaries(export) {
            for code in summary.code {
                export.executions.push(meada_core::export::Execution {
                    workload: format!("{}::{path}::{}", repo.identity, code.address),
                    root: code.root,
                    repository: repo.identity.clone(),
                    path: code.path,
                    level: match code.inferred {
                        true => Level::Inferred,
                        false => Level::Exact,
                    },
                    reason: code.reason,
                });
            }
        }
        crate::index::sort_providers(&mut export.providers);
        crate::index::sort_consumers(&mut export.consumers);
        export.consumers.dedup_by(|a, b| {
            a.site == b.site
                && a.contract == b.contract
                && a.via == b.via
                && a.root == b.root
                && (a.identity == b.identity
                    || a.contract
                        .as_deref()
                        .is_some_and(|c| !crate::contract::unresolved(c)))
        });
    }
    fn workspace_step(&self, exports: &mut [Export], repositories: &[Repository]) {
        patterns::prepare(exports);
        environment::resolve(exports, repositories);
    }
    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
        exports: &[Export],
    ) -> Vec<Link> {
        let Some(contract) = consumer
            .contract
            .as_deref()
            .filter(|c| c.starts_with("image::"))
        else {
            return Vec::new();
        };
        if exports
            .iter()
            .find(|e| e.repository == from.identity)
            .is_some_and(|e| e.executions.iter().any(|x| runs_locally(x, consumer)))
        {
            return Vec::new();
        }
        let mut candidates = std::collections::BTreeMap::new();
        for (builder, p) in providers
            .iter()
            .filter(|(_, p)| matches(&p.contract, contract))
        {
            let evidence = consumer
                .via
                .as_deref()
                .into_iter()
                .chain(p.evidence.as_deref())
                .collect::<Vec<_>>();
            let pattern = p.contract.contains('*') || contract.contains('*');
            let mut level = if evidence.is_empty() && !pattern {
                Level::Exact
            } else {
                Level::Inferred
            };
            let mut reason: String = if evidence.is_empty() {
                contract.into()
            } else {
                format!("{contract}; {}", evidence.join("; "))
            };
            if pattern {
                reason.push_str(&format!("; pattern {} on {contract}", p.contract));
            }
            let mut attribution = None;
            if let Some(choice) = patterns::builder(exports, p) {
                match choice.selected() {
                    Some((selected, decision)) => {
                        if selected != contract {
                            continue;
                        }
                        level = Level::Decided;
                        reason = decision.reason();
                        attribution = Some((decision.by.clone(), decision.p));
                    }
                    None => {
                        level = Level::Ambiguous;
                        reason.push_str("; multiple literal runner names");
                    }
                }
            }
            let mut link = Link::new(
                consumer.site.clone(),
                p.site.clone(),
                contract.into(),
                level,
                reason,
                None,
                None,
                None,
            );
            link.root = consumer.root.clone();
            if let Some((by, p)) = attribution {
                link.by = Some(by);
                link.p = p;
            }
            // A repository builds an image in each CI step that names it: one link a site.
            let key = (
                builder.identity.as_str(),
                p.site.clone(),
                p.contract.clone(),
            );
            // With whether the builder's own name is evidence, not a literal.
            let found = (p.evidence.is_some(), link);
            let prior = candidates.entry(key).or_insert_with(|| found.clone());
            if (found.1.level, &found.1.reason) < (prior.1.level, &prior.1.reason) {
                *prior = found;
            }
        }
        // A builder repository's literal name covers its pattern that matches the same runner.
        let literal = (candidates.keys())
            .filter(|(_, _, c)| !c.contains('*'))
            .map(|&(repository, ..)| repository)
            .collect::<std::collections::BTreeSet<_>>();
        candidates
            .retain(|(repository, _, c), _| !c.contains('*') || !literal.contains(repository));
        // Of the sites of one repository that build one image, those on its best evidence
        // link: a Compose default beside a CI step that names the image adds no link.
        let rank = |(weak, link): &(bool, Link)| (link.level, *weak);
        let mut best = std::collections::HashMap::new();
        for ((repository, _, contract), found) in &candidates {
            let held = (best.entry((*repository, contract.clone()))).or_insert(rank(found));
            *held = rank(found).min(*held);
        }
        candidates.retain(|(repository, _, contract), found| {
            best[&(*repository, contract.clone())] == rank(found)
        });
        // Several CI steps of a repository that build one image are one candidate.
        let builders = (candidates.keys())
            .map(|(repository, _, contract)| (*repository, contract.as_str()))
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let mut links: Vec<Link> = candidates.into_values().map(|(_, link)| link).collect();
        if builders > 1 {
            for l in &mut links {
                l.level = Level::Ambiguous;
                l.by = None;
                l.p = None;
                l.reason.push_str("; multiple builder repositories");
            }
        }
        links
    }
    fn unresolved(&self, contract: &str) -> bool {
        contract
            .strip_prefix("image::")
            .is_some_and(|name| name.contains('*') && !crate::iac::pattern::supported(name))
    }
    fn consumer_item(&self, link: &Link, exports: &[Export]) -> bool {
        patterns::consumer_item(link, exports)
    }
    fn owns_decision(&self, decision: &crate::decisions::Decision) -> bool {
        patterns::handles(decision)
    }
    fn decision_items(&self, exports: &[Export]) -> Vec<(crate::items::OpenItem, bool)> {
        patterns::items(exports)
    }
    fn decision_reports(
        &self,
        workspace: &crate::workspace::Workspace,
        exports: &[Export],
    ) -> Vec<crate::decisions::Evaluation> {
        patterns::reports(workspace, exports)
    }
    fn serves(&self, provider: &Provider, contract: &str) -> bool {
        matches(&provider.contract, contract)
    }
    fn link_candidate(&self, provider: &Provider, link: &Link, exports: &[Export]) -> bool {
        provider.site == link.provider && patterns::active_match(exports, provider, &link.contract)
    }
    fn side_note(&self, exports: &[Export], contract: &str, role: Role) -> &'static str {
        let (built, run) = roles(exports, contract);
        match role {
            Role::Provider if !run => " (no runner in the workspace)",
            Role::Consumer if !built => " (no builder in the workspace)",
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
        // Asked per group: a side whose group lacks a builder or a runner is no orphan. A
        // literal image has its group's roles, asked once per group.
        let mut asked = std::collections::HashMap::<String, (bool, bool)>::new();
        let mut scoped = |contract: &str| {
            if !resolved(contract) {
                return false;
            }
            let key = match contract.contains('*') {
                true => contract,
                false => group(contract),
            };
            let found = match asked.get(key) {
                Some(found) => *found,
                None => *asked.entry(key.into()).or_insert(roles(exports, contract)),
            };
            found == (true, true)
        };
        let mut rows = std::collections::BTreeSet::new();
        for p in export.providers.iter().filter(|p| scoped(&p.contract)) {
            if !exports.iter().flat_map(|e| &e.consumers).any(|c| {
                c.contract
                    .as_deref()
                    .is_some_and(|c| patterns::active_match(exports, p, c))
            }) {
                rows.insert(
                    crate::msg::Msg::OrphanCounterpart {
                        contract: &p.contract,
                        place: p.at.as_deref().unwrap_or("(repository-level)"),
                        builder: false,
                    }
                    .text(),
                );
            }
        }
        for c in
            (export.consumers.iter()).filter(|c| c.contract.as_deref().is_some_and(&mut scoped))
        {
            let contract = c.contract.as_deref().unwrap_or_default();
            if !exports
                .iter()
                .flat_map(|e| &e.providers)
                .any(|p| patterns::active_match(exports, p, contract))
            {
                rows.insert(
                    crate::msg::Msg::OrphanCounterpart {
                        contract,
                        place: c
                            .site
                            .strip_prefix(&format!("{}::", export.repository))
                            .unwrap_or(&c.site),
                        builder: true,
                    }
                    .text(),
                );
            }
        }
        rows.into_iter().collect()
    }
    fn executions(&self, exports: &[Export], links: &[Link]) -> Vec<meada_core::export::Execution> {
        let mut out = exports
            .iter()
            .flat_map(|e| e.executions.iter().cloned())
            .collect::<Vec<_>>();
        // The CI steps of one repository that build an image run its code once, at the best
        // level of their links.
        let mut run = std::collections::HashMap::new();
        for link in links.iter().filter(|l| l.contract.starts_with("image::")) {
            for e in exports {
                for n in &e.iac {
                    let identity = n.identity(&e.repository);
                    if link.consumer == identity
                        && link.root.as_deref().is_none_or(|r| r == n.root)
                        && runs(e, n, &link.contract)
                    {
                        let repository = repository(&link.provider);
                        let key = (identity.clone(), &n.root, repository, &link.contract);
                        let at = *run.entry(key).or_insert(out.len());
                        let execution = meada_core::export::Execution {
                            workload: identity,
                            root: n.root.clone(),
                            repository: repository.into(),
                            path: ".".into(),
                            level: link.level,
                            reason: link.reason.clone(),
                        };
                        match out.get_mut(at) {
                            Some(prior)
                                if (prior.level, &prior.reason) <= (link.level, &link.reason) => {}
                            Some(prior) => *prior = execution,
                            None => out.push(execution),
                        }
                    }
                }
            }
        }
        out
    }
    fn context_at(
        &self,
        exports: &[Export],
        _: &[Repository],
        links: &[Link],
        symbol: &str,
        root: Option<&str>,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        for e in exports {
            for c in e.consumers.iter().filter(|c| {
                c.site == symbol
                    && root.is_none_or(|r| c.root.as_deref() == Some(r))
                    && c.contract
                        .as_deref()
                        .is_some_and(|c| c.starts_with("image::"))
            }) {
                let contract = c.contract.as_deref().unwrap_or_default();
                if root.is_some_and(|root| {
                    !e.iac.iter().any(|n| {
                        n.root == root
                            && n.identity(&e.repository) == symbol
                            && runs(e, n, contract)
                    })
                }) {
                    continue;
                }
                lines.push(format!(
                    "image: {contract}{}",
                    self.side_note(exports, contract, Role::Consumer)
                ));
                for link in links
                    .iter()
                    .filter(|l| l.consumer == symbol && l.contract == contract && l.root == c.root)
                {
                    lines.push(format!(
                        "  builder: {} [{}: {}]",
                        link.provider,
                        link.level.name(),
                        link.reason
                    ));
                }
            }
            // A builder at the node, a CI step: the image it builds and what runs it.
            let built = |p: &&Provider| p.site == symbol && p.contract.starts_with("image::");
            for p in e.providers.iter().filter(built) {
                lines.push(format!("builds {}", p.contract));
                for link in (links.iter())
                    .filter(|l| l.provider == symbol && matches(&p.contract, &l.contract))
                {
                    let root = link.root.as_deref();
                    let root = root.map_or(String::new(), |root| format!(" (root {root})"));
                    lines.push(format!(
                        "  runner: {}{root} [{}: {}]",
                        link.consumer,
                        link.level.name(),
                        link.reason
                    ));
                }
            }
            for n in &e.iac {
                let identity = n.identity(&e.repository);
                if identity == symbol && root.is_none_or(|r| r == n.root) {
                    for entry in &n.environment {
                        lines.push(format!(
                            "env {} {} -> {}: {}",
                            entry.container,
                            entry.name,
                            entry.target,
                            entry
                                .value
                                .as_deref()
                                .or(entry.contract.as_deref())
                                .unwrap_or("unresolved")
                        ));
                    }
                }
                if identity == symbol && root.is_none_or(|r| r == n.root) {
                    for execution in e
                        .executions
                        .iter()
                        .filter(|x| x.workload == identity && x.root == n.root)
                    {
                        lines.push(format!(
                            "code: {} {} [{}]",
                            execution.repository,
                            execution.path,
                            execution.level.name()
                        ));
                    }
                }
            }
        }
        lines
    }
}

/// The repository of a builder's site: the site itself, or the repository of its CI step.
fn repository(site: &str) -> &str {
    site.split_once("::")
        .map_or(site, |(repository, _)| repository)
}

/// The CI step that holds the builder form at `at` (`<file>#<key path>`), when the CI reader
/// gives the file its nodes.
fn step(export: &Export, at: &str) -> Option<String> {
    let (path, key) = at.split_once('#')?;
    (export.iac.iter())
        .filter(|n| n.ty == crate::iac::actions::STEP && n.path == path)
        .find(|n| (key.strip_prefix(n.address.as_str())).is_some_and(|rest| rest.starts_with('.')))
        .map(|n| n.identity(&export.repository))
}

/// The options of `docker push` that take no value. The word after any other option is its
/// value, never an image: a flag missing here costs the image after it, and gives no wrong one.
const PUSH_FLAGS: [&str; 5] = [
    "-a",
    "--all-tags",
    "-q",
    "--quiet",
    "--disable-content-trust",
];

/// The images a command of a `run` script publishes (ADR 0028, the command table): the one
/// `docker push` names, and each `-t` or `--tag` of a `docker buildx build` with `--push`.
/// No other word of the command is read for a name.
fn pushed(command: &[Word]) -> Vec<String> {
    let words: Vec<Option<&str>> = command.iter().map(|w| literal(w)).collect();
    let mut tags = Vec::new();
    match words.as_slice() {
        [Some("docker"), Some("push"), rest @ ..] => {
            let mut rest = rest.iter().zip(&command[2..]);
            while let Some((text, word)) = rest.next() {
                match text {
                    Some(option) if option.starts_with('-') => {
                        if !PUSH_FLAGS.contains(option) && !option.contains('=') {
                            rest.next();
                        }
                    }
                    _ => tags.push((word, 0)),
                }
            }
            tags.truncate(usize::from(tags.len() == 1));
        }
        [Some("docker"), Some("buildx"), Some("build"), ..] => {
            // A `--push` that is the value of a tag option pushes nothing.
            let mut push = false;
            let mut rest = command[3..].iter();
            while let Some(word) = rest.next() {
                match word.as_slice() {
                    [Part::Text(option)] if option == "--push" => push = true,
                    [Part::Text(option)] if option == "-t" || option == "--tag" => {
                        tags.extend(rest.next().map(|tag| (tag, 0)));
                    }
                    [Part::Text(option), ..] if option.starts_with("--tag=") => {
                        tags.push((word, "--tag=".len()));
                    }
                    _ => {}
                }
            }
            if !push {
                return Vec::new();
            }
        }
        _ => {}
    }
    let mut images: Vec<String> = (tags.into_iter())
        .filter_map(|(word, from)| named(word, from))
        .collect();
    images.sort();
    images.dedup();
    images
}

/// The image the word names from its byte `from` on: an open part is the `*` of a pattern, and
/// a name left open whole is `*`, an unresolved side.
fn named(word: &[Part], from: usize) -> Option<String> {
    let mut name = String::new();
    for part in word {
        match part {
            Part::Text(text) if text.contains('*') => return None,
            Part::Text(text) => name += text,
            Part::Open(_) if name.ends_with('*') => {}
            Part::Open(_) => name.push('*'),
        }
    }
    // A name that starts as an option is none: Docker refuses it.
    normalize_name(
        name.get(from..).filter(|name| !name.starts_with('-'))?,
        true,
    )
}

/// Whether `execution` runs a local path for the workload side `consumer` in its root: a module
/// run from a local path in one root may run an image in another.
fn runs_locally(execution: &meada_core::export::Execution, consumer: &Consumer) -> bool {
    execution.workload == consumer.site
        && (consumer.root.as_ref()).is_none_or(|root| *root == execution.root)
}

fn runs(export: &Export, node: &crate::iac::Node, contract: &str) -> bool {
    if node.ty == "HelmValues" {
        return summaries(export)
            .iter()
            .filter(|(p, _)| *p == node.path)
            .flat_map(|(_, s)| &s.runners)
            .any(|r| format!("image::{}", r.image) == contract);
    }
    images(export)
        .into_iter()
        .filter(|(n, _, _)| n.root == node.root && n.address == node.address)
        .any(|(_, image, _)| evaluated(image).is_some_and(|i| format!("image::{i}") == contract))
}

/// The one projection of stored format facts into image runners.
fn images(export: &Export) -> Vec<(&crate::iac::Node, &str, Option<String>)> {
    let mut out = Vec::new();
    let mut nodes = std::collections::BTreeMap::new();
    for node in &export.iac {
        nodes.insert((node.root.as_str(), node.address.as_str()), node);
        if node.ty == "service"
            && crate::iac::types::workload("compose", &node.ty).is_some()
            && let Some(image) = &node.image
        {
            out.push((
                node,
                image.text.as_str(),
                image.overridable.then(|| {
                    format!(
                        "image: {}#{}.image (Compose default)",
                        node.path, node.block
                    )
                }),
            ));
        }
        for image in &node.images {
            let via = (!image.through.is_empty())
                .then(|| {
                    let values = image
                        .through
                        .iter()
                        .map(|o| format!("{} {}:{}", o.variable, o.path, o.line))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("image: {}#{} ({values})", node.path, node.block)
                })
                .or_else(|| {
                    image
                        .image
                        .overridable
                        .then(|| format!("image: {}#{} (default)", node.path, node.block))
                });
            out.push((node, image.image.text.as_str(), via));
        }
    }
    for (root, address, image) in &export.workload_images {
        if let Some(node) = nodes.get(&(root.as_str(), address.as_str()))
            && crate::iac::types::workload("kubernetes", &node.ty)
                .is_some_and(|w| !w.images.is_empty())
        {
            out.push((*node, image.as_str(), None));
        }
    }
    out
}

/// Whether the scope holds a builder and a runner of a resolved image name in the group of
/// `contract`, the image's orphan scope (ADR 0026, "Orphan sides").
fn roles(exports: &[Export], contract: &str) -> (bool, bool) {
    let side = |c: &str| resolved(c) && same_group(c, contract);
    (
        (exports.iter().flat_map(|e| &e.providers)).any(|p| side(&p.contract)),
        (exports.iter().flat_map(|e| &e.consumers))
            .any(|c| c.contract.as_deref().is_some_and(side)),
    )
}

fn summaries(export: &Export) -> Vec<(String, config::Summary)> {
    export
        .configurations
        .iter()
        .filter_map(|(p, s)| {
            Some((
                p.clone(),
                serde_json::from_value(s.get("image")?.clone()).ok()?,
            ))
        })
        .collect()
}

fn runner(
    repository: &str,
    path: &str,
    address: &str,
    name: &str,
    root: &str,
    via: Option<String>,
) -> Consumer {
    Consumer {
        identity: None,
        contract: Some(format!("image::{name}")),
        site: format!("{repository}::{path}::{address}"),
        via,
        shape: None,
        method: None,
        fragments: Vec::new(),
        hops: Vec::new(),
        root: Some(root.into()),
        lead: None,
    }
}

fn matches(left: &str, right: &str) -> bool {
    match (left.strip_prefix("image::"), right.strip_prefix("image::")) {
        (Some(a), Some(b)) if a.contains('*') => crate::iac::pattern::matches(a, b),
        (Some(a), Some(b)) if b.contains('*') => crate::iac::pattern::matches(b, a),
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Unresolved config interpolation keeps a pattern, without evaluating a process environment.
fn evaluated(written: &str) -> Option<String> {
    if let Some(name) = normalize(written) {
        return Some(name);
    }
    let mut out = String::new();
    let mut rest = written;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        if rest.starts_with('{') {
            let mut depth = 0usize;
            let end = rest.char_indices().find_map(|(i, c)| {
                match c {
                    '{' => depth += 1,
                    '}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
                (depth == 0).then_some(i + c.len_utf8())
            })?;
            rest = &rest[end..];
        } else {
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            if end == 0 {
                return None;
            }
            rest = &rest[end..];
        }
        if !out.ends_with('*') {
            out.push('*');
        }
    }
    out.push_str(rest);
    normalize_name(&out, true)
}

/// The registry and namespace an image's orphan scope is asked in: the registry and the first
/// segment of the repository path (`ghcr.io/acme/shop` and `ghcr.io/acme/tools/lint` are in
/// `ghcr.io/acme`), or the registry alone for a path of one segment.
fn group(contract: &str) -> &str {
    let name = contract.strip_prefix("image::").unwrap_or(contract);
    let Some((registry, path)) = name.split_once('/') else {
        return name;
    };
    match path.split_once('/') {
        Some((namespace, _)) => &name[..registry.len() + 1 + namespace.len()],
        None => registry,
    }
}

/// Whether two images are in one group. A name is in its own group; a pattern (a name with an
/// unresolved part, `*`) is in every group whose names its text before the `*` can start:
/// `ghcr.io/*` is in `ghcr.io/acme` and in `ghcr.io` (names of one segment), `ghcr.io/acme-*` is
/// not in `ghcr.io/acme`, nor `ghcr.io/acme/*` in `ghcr.io`.
fn same_group(a: &str, b: &str) -> bool {
    // What the names of an image's groups start with, and whether they end one segment later
    // (the group of a registry alone).
    let start = |c: &str| {
        let name = c.strip_prefix("image::").unwrap_or(c);
        match name.split_once('*') {
            Some((start, _)) => (start.to_string(), false),
            None => (format!("{}/", group(name)), !group(name).contains('/')),
        }
    };
    let (a, b) = (start(a), start(b));
    let ((short, single), (long, _)) = match a.0.len() <= b.0.len() {
        true => (a, b),
        false => (b, a),
    };
    if let Some(rest) = long.strip_prefix(short.as_str()) {
        return !single || !rest.contains('/');
    }
    // Starts that part ways share a group only where what they share already holds one: a
    // registry and a namespace, or a registry while neither start goes past one segment.
    let shared = (short.bytes().zip(long.bytes()))
        .take_while(|(x, y)| x == y)
        .filter(|(x, _)| *x == b'/')
        .count();
    match shared {
        0 => false,
        1 => short.matches('/').count() == 1 && long.matches('/').count() == 1,
        _ => true,
    }
}

/// Whether `contract` is an image the kind resolved, a name or a pattern it links by.
fn resolved(contract: &str) -> bool {
    contract.starts_with("image::") && !crate::contract::unresolved(contract)
}

/// The registry and repository of an image, without its deployment tag or digest.
fn normalize(written: &str) -> Option<String> {
    normalize_name(written, false)
}

fn normalize_name(written: &str, pattern: bool) -> Option<String> {
    let written = written.trim();
    if written.is_empty() || written.contains("://") {
        return None;
    }
    let name = written.split('@').next()?;
    let slash = name.rfind('/');
    let name = match name.rfind(':') {
        Some(at) if slash.is_none_or(|slash| at > slash) => &name[..at],
        _ => name,
    }
    .to_ascii_lowercase();
    if name.is_empty()
        || name.contains(['$', '{', '}'])
        || (!pattern && name.contains('*'))
        || name.chars().any(char::is_whitespace)
    {
        return None;
    }
    if name
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/:".contains(&b) || (pattern && b == b'*'))
    {
        return None;
    }
    // Normalization must not invent a pattern's literal evidence by adding Docker Hub's
    // default registry and namespace to an unknown or unsupported source name.
    if pattern && name.contains('*') && !crate::iac::pattern::supported(&name) {
        return Some(name);
    }
    let (first, rest) = name.split_once('/').unwrap_or((&name, ""));
    // An unresolved first component may be a registry. Docker Hub's default would
    // turn that uncertainty into a different pattern and hide literal candidates.
    if first.contains('*') {
        return Some(name);
    }
    let (registry, repository) = if rest.is_empty() {
        ("docker.io", format!("library/{first}"))
    } else if first.contains(['.', ':']) || first == "localhost" {
        let registry = match first {
            "index.docker.io" | "registry-1.docker.io" => "docker.io",
            _ => first,
        };
        let repository = if registry == "docker.io" && !rest.contains('/') {
            format!("library/{rest}")
        } else {
            rest.into()
        };
        (registry, repository)
    } else {
        ("docker.io", name.clone())
    };
    Some(format!("{registry}/{repository}"))
}

#[cfg(test)]
mod tests {
    use super::{evaluated, group, matches, normalize, pushed, same_group};

    #[test]
    fn a_command_publishes_the_image_a_push_names_and_each_tag_of_a_build_that_pushes() {
        let env = |name: &str| (name == "IMAGE").then_some("ghcr.io/acme/orders");
        let published = |script: &str| -> Vec<String> {
            let commands = crate::iac::actions::run::commands(script, env).unwrap();
            commands.iter().flat_map(|c| pushed(c)).collect()
        };
        let cases: &[(&str, &[&str])] = &[
            (
                "docker push ghcr.io/acme/orders:v1",
                &["ghcr.io/acme/orders"],
            ),
            ("docker push Acme/API", &["docker.io/acme/api"]),
            (
                "docker push -q --all-tags \"$IMAGE\"",
                &["ghcr.io/acme/orders"],
            ),
            (
                "docker push --platform linux/amd64 $IMAGE:$TAG",
                &["ghcr.io/acme/orders"],
            ),
            (
                "docker push --platform=linux/amd64 ${{ env.IMAGE }}@$DIGEST",
                &["ghcr.io/acme/orders"],
            ),
            // A part of the name left open is a pattern, the whole name an unresolved side.
            (
                "docker push ghcr.io/$OWNER/orders:v1",
                &["ghcr.io/*/orders"],
            ),
            ("docker push $OTHER", &["*"]),
            ("docker push ${{ inputs.image }}:${{ github.sha }}", &["*"]),
            // The word after an option the row does not know is its value, and two names fit
            // no `docker push`.
            ("docker push --unknown ghcr.io/acme/orders", &[]),
            ("docker push ghcr.io/acme/a ghcr.io/acme/b", &[]),
            ("docker push", &[]),
            ("docker push 'ghcr.io/acme/*'", &[]),
            ("docker push https://ghcr.io/acme/orders", &[]),
            ("docker image push ghcr.io/acme/orders", &[]),
            ("echo docker push ghcr.io/acme/orders", &[]),
            // A build publishes only with `--push`, and only what its tag options name.
            ("docker build -t ghcr.io/acme/orders .", &[]),
            ("docker build --push -t ghcr.io/acme/orders .", &[]),
            ("docker buildx build -t ghcr.io/acme/orders .", &[]),
            ("docker buildx build --push .", &[]),
            (
                "docker buildx build --output type=registry -t ghcr.io/acme/orders .",
                &[],
            ),
            (
                "docker buildx build --push -t ghcr.io/acme/b:v1 --tag ghcr.io/acme/a \
                 --tag=$IMAGE:${{ github.sha }} .",
                &["ghcr.io/acme/a", "ghcr.io/acme/b", "ghcr.io/acme/orders"],
            ),
            (
                "docker buildx build -t ghcr.io/acme/orders -t ghcr.io/acme/orders:v1 \
                 --build-arg BASE=ghcr.io/$OWNER/base --label image=$OTHER --push .",
                &["ghcr.io/acme/orders"],
            ),
            ("docker buildx build --push -t $OTHER .", &["*"]),
            // A tag option takes the word after it, also `--push` (found by the verifier of
            // #833): nothing is pushed, and an option is no image name.
            ("docker buildx build -t --push .", &[]),
            ("docker buildx build --tag --push .", &[]),
            ("docker buildx build --push -t --load .", &[]),
            ("docker buildx build --push --tag=-x .", &[]),
            ("docker push -- -x", &[]),
            (
                "docker buildx build --push --tag=ghcr.io/acme/${{ matrix.name }} .",
                &["ghcr.io/acme/*"],
            ),
        ];
        for (script, images) in cases {
            assert_eq!(published(script), *images, "{script}");
        }
    }

    #[test]
    fn an_image_is_grouped_by_its_registry_and_namespace() {
        for (contract, expected) in [
            ("image::ghcr.io/acme/shop", "ghcr.io/acme"),
            ("image::ghcr.io/acme/tools/lint", "ghcr.io/acme"),
            ("image::docker.io/library/postgres", "docker.io/library"),
            ("image::localhost:5000/orders", "localhost:5000"),
            ("image::localhost:5000/acme/shop", "localhost:5000/acme"),
        ] {
            assert_eq!(group(contract), expected, "{contract}");
        }
        let shop = "image::ghcr.io/acme/shop";
        let redis = "image::docker.io/library/redis";
        assert!(same_group(shop, "image::ghcr.io/acme/tools/lint"));
        assert!(!same_group(shop, redis));
        assert!(!same_group(shop, "image::ghcr.io/other/shop"));
        // A pattern is in the groups its text before the `*` can start.
        assert!(same_group("image::ghcr.io/*/orders", shop));
        assert!(same_group("image::ghcr.io/*", shop));
        assert!(same_group("image::ghcr.io/acme/orders-*", shop));
        assert!(same_group(shop, "image::ghcr.io/acme/orders-*"));
        assert!(!same_group("image::ghcr.io/acme-*", shop));
        assert!(same_group(
            "image::ghcr.io/acme-*",
            "image::ghcr.io/acme-tools/lint"
        ));
        assert!(!same_group("image::ghcr.io/*/orders", redis));
        // The group of one-segment names takes a pattern only while it stays in one segment.
        let orders = "image::localhost:5000/orders";
        assert!(same_group("image::localhost:5000/*", orders));
        assert!(same_group(orders, "image::localhost:5000/ord-*"));
        assert!(!same_group("image::localhost:5000/acme/*", orders));
        assert!(!same_group(orders, "image::localhost:5000/acme/*"));
        assert!(!same_group("image::localhost:5000/acme/shop", orders));
        // Two patterns share a group when one start leads to the other, or when what they share
        // holds a registry and a namespace, or a registry while neither goes past one segment.
        assert!(same_group(
            "image::ghcr.io/acme/orders-*",
            "image::ghcr.io/acme/billing-*"
        ));
        assert!(same_group("image::ghcr.io/acme-*", "image::ghcr.io/zeta-*"));
        assert!(!same_group(
            "image::ghcr.io/acme/*",
            "image::ghcr.io/zeta-*"
        ));
        assert!(!same_group(
            "image::ghcr.io/acme/*",
            "image::docker.io/acme/*"
        ));
    }

    #[test]
    fn a_wholly_unknown_image_has_no_literal_matching_evidence() {
        assert_eq!(evaluated("${IMAGE}").as_deref(), Some("*"));
        assert!(!matches("image::*", "image::docker.io/library/nginx"));
        assert!(!matches("image::*", "image::*"));
    }

    #[test]
    fn image_names_expand_docker_hub_and_drop_tags_and_digests() {
        for (written, name) in [
            ("nginx:1.27", "docker.io/library/nginx"),
            ("Acme/API:latest", "docker.io/acme/api"),
            ("docker.io/nginx", "docker.io/library/nginx"),
            (
                "index.docker.io/library/nginx@sha256:abc",
                "docker.io/library/nginx",
            ),
            ("GHCR.IO/Acme/Orders:v1@sha256:abc", "ghcr.io/acme/orders"),
            ("localhost:5000/orders:dev", "localhost:5000/orders"),
            ("registry.acme.io:5000/a/b", "registry.acme.io:5000/a/b"),
        ] {
            assert_eq!(normalize(written).as_deref(), Some(name), "{written}");
        }
        for written in [
            "",
            "https://ghcr.io/acme/orders",
            "${IMAGE}",
            "ghcr.io/${ORG}/api",
            "bad name",
            "/api",
            "acme//api",
            "api/../other",
        ] {
            assert_eq!(normalize(written), None, "{written}");
        }
    }
}
