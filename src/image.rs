//! Images connect workload runners to the repositories whose configuration builds them.

use crate::contract::Level;
use crate::contract::{ContractKind, Found, Link, Role};
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::resolve::Code;
use crate::workspace::Repository;
use serde_json::Value;

pub(crate) mod code;
mod config;
pub(crate) mod environment;
mod jib;
mod patterns;

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
    fn config_reads_paths(&self, path: &str) -> bool {
        crate::iac::is_source(path) || crate::iac::cloudformation::is_source(path)
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
                    site: repo.identity.clone(),
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
        for n in &export.iac {
            if let Some(path) = direct(export, n) {
                export.executions.push(meada_core::export::Execution {
                    workload: n.identity(&repo.identity),
                    root: n.root.clone(),
                    repository: repo.identity.clone(),
                    path: path.into(),
                    level: Level::Exact,
                    reason: format!("Compose build {}#{}.build", n.path, n.block),
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
                    level: Level::Exact,
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
        for (_, p) in providers
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
            let key = (p.site.clone(), p.contract.clone());
            let prior = candidates.entry(key).or_insert_with(|| link.clone());
            if (link.level, &link.reason) < (prior.level, &prior.reason) {
                *prior = link;
            }
        }
        let mut links = candidates.into_values().collect::<Vec<_>>();
        if links.len() > 1 {
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
    fn side_note(&self, exports: &[Export], _: &str, role: Role) -> &'static str {
        let (built, run) = roles(exports);
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
        if roles(exports) != (true, true) {
            return Vec::new();
        }
        let mut rows = std::collections::BTreeSet::new();
        for p in export
            .providers
            .iter()
            .filter(|p| p.contract.starts_with("image::"))
        {
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
        for c in export.consumers.iter().filter(|c| {
            c.contract
                .as_deref()
                .is_some_and(|c| c.starts_with("image::"))
        }) {
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
        for link in links.iter().filter(|l| l.contract.starts_with("image::")) {
            for e in exports {
                for n in &e.iac {
                    let identity = n.identity(&e.repository);
                    if link.consumer == identity
                        && link.root.as_deref().is_none_or(|r| r == n.root)
                        && runs(e, n, &link.contract)
                    {
                        out.push(meada_core::export::Execution {
                            workload: identity,
                            root: n.root.clone(),
                            repository: link.provider.clone(),
                            path: ".".into(),
                            level: link.level,
                            reason: link.reason.clone(),
                        });
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
                            "code: {} {} [exact]",
                            execution.repository, execution.path
                        ));
                    }
                }
            }
        }
        lines
    }
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

fn roles(exports: &[Export]) -> (bool, bool) {
    (
        exports
            .iter()
            .flat_map(|e| &e.providers)
            .any(|p| p.contract.starts_with("image::")),
        exports.iter().flat_map(|e| &e.consumers).any(|c| {
            c.contract
                .as_deref()
                .is_some_and(|c| c.starts_with("image::"))
        }),
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

fn direct<'a>(export: &Export, node: &'a crate::iac::Node) -> Option<&'a str> {
    let path = node.build.as_ref()?.text.as_str();
    let summaries = summaries(export);
    (crate::iac::types::workload("compose", &node.ty).is_some_and(|w| w.code.is_some())
        && summaries
            .iter()
            .any(|(_, s)| s.paths.iter().any(|p| p == path)))
    .then_some(path)
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
    use super::{evaluated, matches, normalize};

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
