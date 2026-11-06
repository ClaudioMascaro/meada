//! Provisioner sides and their links (ADR 0026): derived from stored IaC names and the
//! workspace's contract sides, never mixed with consumer-to-provider links.

use std::collections::HashMap;

use meada_core::export::{IacNode, Level, Provision, Provisioner};
use rusqlite::Connection;

use super::{Node, types};
use crate::Result;
use crate::index::{Export, Span};

/// A provisioner with the evaluated name's evidence, kept only during a query.
pub(crate) struct Side {
    pub identity: String,
    pub contract: String,
    pub node: Node,
}

impl Side {
    pub(crate) fn pattern(&self) -> bool {
        self.node
            .provisioned
            .as_ref()
            .map_or(self.node.pattern, |p| p.pattern)
    }

    pub(crate) fn overridable(&self) -> &[super::Overridable] {
        self.node
            .provisioned
            .as_ref()
            .map_or(&self.node.overridable, |p| &p.through)
    }

    pub(crate) fn matches(&self, contract: &str) -> bool {
        // A written asterisk is not an unresolved part when evaluation says the name is
        // literal. Preserve its literal identity instead of inventing wildcard evidence.
        if !self.pattern() && self.contract.contains('*') {
            return self.contract == contract;
        }
        let (Some((kind, pattern)), Some((other, name))) =
            (self.contract.rsplit_once("::"), contract.rsplit_once("::"))
        else {
            return false;
        };
        kind == other
            && match (pattern.contains('*'), name.contains('*')) {
                (true, false) => super::pattern::matches(pattern, name),
                (false, true) => super::pattern::matches(name, pattern),
                (false, false) => pattern == name,
                (true, true) => false,
            }
    }

    pub(crate) fn place(&self, repository: &str) -> String {
        format!(
            "{repository} {}:{} {} (root {})",
            self.node.path, self.node.line, self.node.address, self.node.root
        )
    }

    pub(crate) fn written(&self) -> (IacNode, Provisioner) {
        let mut node = IacNode::default();
        node.identity = self.identity.clone();
        node.address = self.node.address.clone();
        node.ty = self.node.ty.clone();
        node.root = self.node.root.clone();
        node.path = self.node.path.clone();
        node.line = self.node.line;
        node.end_line = self.node.end_line;
        let mut side = Provisioner::default();
        side.contract = self.contract.clone();
        side.node = self.identity.clone();
        side.root = self.node.root.clone();
        (node, side)
    }
}

/// Only resource blocks of known types with a physical name provision contracts.
/// A data block names an existing resource; it does not create one.
pub(crate) fn read(db: &Connection, repository: &str) -> Result<Vec<Side>> {
    let mut statement = db.prepare(&format!("SELECT {} FROM iac_nodes", super::COLUMNS))?;
    let mut sides = Vec::new();
    for node in statement.query_map([], super::row)? {
        let node = node?;
        let Some(entry) = types::provision_entry(&node.path, &node.block, &node.ty) else {
            continue;
        };
        let target = node
            .provisioned
            .as_ref()
            .map_or(node.name.as_deref(), |p| p.name.as_deref());
        let Some(name) = target.filter(|n| !n.is_empty()) else {
            continue;
        };
        // A structured identifier the entry could not read is not a physical AWS name.
        // It may name another service, so never invent a typed provisioner for it.
        if name.starts_with("arn:") || name.contains("://") {
            continue;
        }
        if node
            .provisioned
            .as_ref()
            .map_or(node.pattern, |p| p.pattern)
            && !super::pattern::supported(name)
        {
            continue;
        }
        sides.push(Side {
            identity: format!("{repository}::{}::{}", node.path, node.address),
            contract: entry.contract(name),
            node,
        });
    }
    sides.sort_by(|a, b| {
        (&a.contract, &a.identity, &a.node.root).cmp(&(&b.contract, &b.identity, &b.node.root))
    });
    Ok(sides)
}

/// Directed and directionless uses both reach provisioners; neither invents a direction.
pub(crate) fn uses(export: &Export) -> impl Iterator<Item = (&str, &str)> {
    directed_uses(export)
        .map(|(site, contract, _)| (site, contract))
        .chain(
            export
                .config_uses
                .iter()
                .map(|u| (u.site.as_str(), u.contract.as_str())),
        )
}

fn directed_uses(export: &Export) -> impl Iterator<Item = (&str, &str, Level)> {
    export
        .providers
        .iter()
        .map(|p| {
            (
                p.site.as_str(),
                p.contract.as_str(),
                crate::contract::use_level(&p.contract, p.via.as_deref(), p.method.as_deref()),
            )
        })
        .chain(export.consumers.iter().filter_map(|c| {
            let contract = c.contract.as_deref()?;
            Some((
                c.site.as_str(),
                contract,
                crate::contract::use_level(contract, c.via.as_deref(), c.method.as_deref()),
            ))
        }))
}

/// Every use of the same contract reaches every provisioner, including one in its repository.
/// Several roots are independent sides, never ambiguous candidates of one another.
pub(crate) fn links(exports: &[Export]) -> Vec<Provision> {
    let mut links = raw_links(exports);
    if !exports.iter().flat_map(|e| &e.decisions.items).any(handles) {
        return links;
    }
    let choices = Choices::new(exports, &links);
    let mut selected = std::collections::BTreeMap::new();
    for export in exports {
        for decision in export.decisions.items.iter().filter(|d| handles(d)) {
            if let Ok(contract) = choices.choose(decision, export, exports) {
                selected.insert(decision.call.as_str(), (contract, decision.reason()));
            }
        }
    }
    links.retain_mut(|link| {
        let Some((id, literal)) = choices.source_of(link) else {
            return true;
        };
        let Some((chosen, reason)) = selected.get(id.as_str()) else {
            return true;
        };
        if literal != *chosen {
            return false;
        }
        link.level = Level::Decided;
        link.reason = reason.clone();
        true
    });
    links.sort();
    links
}

fn raw_links(exports: &[Export]) -> Vec<Provision> {
    let sides = exports
        .iter()
        .flat_map(|export| {
            directed_uses(export).chain(
                export
                    .config_uses
                    .iter()
                    .map(|u| (u.site.as_str(), u.contract.as_str(), u.level)),
            )
        })
        .collect::<Vec<_>>();
    let config = exports
        .iter()
        .flat_map(|e| &e.config_uses)
        .map(|u| ((u.site.as_str(), u.contract.as_str()), u))
        .collect::<std::collections::HashMap<_, _>>();
    let evidence = exports
        .iter()
        .flat_map(|e| {
            e.providers
                .iter()
                .map(|p| (p.site.as_str(), p.contract.as_str(), p.via.as_deref()))
                .chain(e.consumers.iter().filter_map(|c| {
                    Some((c.site.as_str(), c.contract.as_deref()?, c.via.as_deref()))
                }))
        })
        .filter_map(|(site, contract, via)| {
            crate::contract::environment_reason(via).map(|reason| ((site, contract), reason))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut links = Vec::new();
    let mut pattern_names =
        std::collections::BTreeMap::<&str, std::collections::BTreeSet<&str>>::new();
    for export in exports {
        for p in &export.provisioners {
            let mut level = Level::Exact;
            let contracts = sides
                .iter()
                .map(|(_, c, _)| *c)
                .filter(|c| p.matches(c))
                .collect::<std::collections::BTreeSet<_>>();
            for contract in &contracts {
                if contract.contains('*') && !p.contract.contains('*') {
                    pattern_names
                        .entry(contract)
                        .or_default()
                        .insert(&p.contract);
                }
            }
            let mut reason = p.contract.clone();
            if p.pattern() {
                level = if contracts.len() > 1 {
                    Level::Ambiguous
                } else {
                    Level::Inferred
                };
                reason += "; pattern";
            }
            if !p.overridable().is_empty() {
                level = level.max(Level::Inferred);
                let values = p
                    .overridable()
                    .iter()
                    .map(|o| {
                        let from = if o.tfvars { "tfvars" } else { "default" };
                        format!(
                            "{} {}:{} {} ({from})",
                            export.repository, o.path, o.line, o.variable
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                reason += &format!("; overridable: {values}");
            }
            for &(site, contract, use_level) in &sides {
                if contracts.contains(contract) {
                    let mut use_reason = match config.get(&(site, contract)) {
                        Some(u) => format!("{reason}; config {}#{}", u.path, u.key),
                        None => reason.clone(),
                    };
                    if let Some(reason) = evidence.get(&(site, contract)) {
                        use_reason += &format!("; {reason}");
                    }
                    let mut link = Provision::new(
                        site.into(),
                        p.identity.clone(),
                        p.node.root.clone(),
                        contract.into(),
                        level.max(use_level),
                        use_reason,
                    );
                    if let Some(u) = config.get(&(site, contract)) {
                        link.at = Some(format!("{}#{}", u.path, u.key));
                    }
                    links.push(link);
                }
            }
        }
    }
    for link in &mut links {
        if !pattern_names.contains_key(link.contract.as_str()) {
            continue;
        }
        link.level = link.level.max(
            if pattern_names
                .get(link.contract.as_str())
                .is_some_and(|names| names.len() > 1)
            {
                Level::Ambiguous
            } else {
                Level::Inferred
            },
        );
        link.reason += &format!("; pattern {}", link.contract);
    }
    links.sort();
    links.dedup();
    links
}

/// Either side can hold the unresolved name; the decision belongs to that side.
#[derive(Clone, Copy)]
enum Source<'a> {
    Provisioner(&'a Side),
    Use { site: &'a str, contract: &'a str },
}

/// Source locations are indexed once per repository, before placing any patterned item.
struct Locations<'a> {
    spans: HashMap<&'a str, &'a Span>,
    nodes: HashMap<String, &'a Node>,
    keys: HashMap<&'a str, &'a crate::config_values::sites::Site>,
}

impl<'a> Locations<'a> {
    fn new(export: &'a Export) -> Self {
        let mut spans = HashMap::new();
        for (id, span) in export
            .calls
            .iter()
            .map(|call| (call.id.as_str(), &call.span))
            .chain(
                export
                    .symbols
                    .iter()
                    .map(|symbol| (symbol.id.as_str(), &symbol.span)),
            )
        {
            spans.entry(id).or_insert(span);
        }
        let mut nodes = HashMap::new();
        for node in &export.iac {
            nodes
                .entry(node.identity(&export.repository))
                .or_insert(node);
        }
        let mut keys = HashMap::new();
        for key in &export.config_sites {
            keys.entry(key.id.as_str()).or_insert(key);
        }
        Self { spans, nodes, keys }
    }
}

impl Source<'_> {
    fn id(self) -> String {
        match self {
            Self::Provisioner(p) => format!("provision::{}::root={}", p.identity, p.node.root),
            Self::Use { site, contract } => format!("provision-use::{site}::{contract}"),
        }
    }

    fn target_exists(self, export: &Export, contract: &str) -> bool {
        match self {
            Self::Provisioner(_) => uses(export).any(|(_, c)| c == contract),
            Self::Use { .. } => export.provisioners.iter().any(|p| p.contract == contract),
        }
    }

    fn span(self, locations: &Locations<'_>, files: &mut crate::render::Files) -> Span {
        let site = match self {
            Self::Provisioner(p) => return p.node.span(files.get(&p.node.path)),
            Self::Use { site, .. } => site,
        };
        if let Some(&span) = locations.spans.get(site) {
            return span.clone();
        }
        if let Some(node) = locations.nodes.get(site) {
            return node.span(files.get(&node.path));
        }
        let mut span = Span::default();
        span.path = "meada.toml".into();
        span
    }

    fn placement(
        self,
        locations: &Locations<'_>,
        files: &mut crate::render::Files,
    ) -> (String, Option<usize>, String) {
        if let Self::Use { site, contract } = self
            && let Some(key) = locations.keys.get(site)
        {
            // A source line can contain redacted/cut values, even beside an ordinary key.
            // Keep declaration metadata without rereading any configuration payload.
            return (
                key.path.clone(),
                Some(key.line),
                format!("{contract} at {}#{}", key.path, key.key),
            );
        }
        let span = self.span(locations, files);
        let expression = files
            .get(&span.path)
            .and_then(|s| s.get(span.start as usize..span.end as usize))
            .filter(|s| !s.is_empty())
            .map(|s| {
                String::from_utf8_lossy(s)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| match self {
                Self::Use { contract, .. } => contract.into(),
                Self::Provisioner(p) => p.node.address.clone(),
            });
        let line = files
            .get(&span.path)
            .filter(|_| span.end > span.start)
            .map(|s| crate::render::line_of(s, span.start));
        (span.path, line, expression)
    }
}

fn sources(export: &Export) -> Vec<Source<'_>> {
    let mut sources: Vec<_> = export
        .provisioners
        .iter()
        .map(Source::Provisioner)
        .collect();
    let uses = uses(export)
        .filter(|(_, c)| c.contains('*'))
        .collect::<std::collections::BTreeSet<_>>();
    sources.extend(
        uses.into_iter()
            .map(|(site, contract)| Source::Use { site, contract }),
    );
    sources
}

pub(crate) fn handles(decision: &crate::decisions::Decision) -> bool {
    decision.call.starts_with("provision::") || decision.call.starts_with("provision-use::")
}

pub(crate) fn decision_context(links: &[Provision], identity: &str, root: &str) -> Vec<String> {
    let choices = links
        .iter()
        .filter(|l| l.provisioner == identity && l.root == root && l.level == Level::Decided)
        .map(|l| format!("{} [decided: {}]", l.contract, l.reason))
        .collect::<std::collections::BTreeSet<_>>();
    if choices.is_empty() {
        Vec::new()
    } else {
        crate::render::section("resolved names", choices.into_iter().collect())
    }
}

/// Build source and literal-candidate indexes once, shared by every decision in this pass.
struct Choices<'a> {
    sources: std::collections::BTreeMap<String, (&'a str, Source<'a>)>,
    targets: std::collections::BTreeMap<(&'a str, &'a str), (&'a str, &'a Side)>,
    candidates: std::collections::BTreeMap<String, Vec<crate::items::Candidate>>,
    ambiguous: std::collections::BTreeSet<String>,
}

impl<'a> Choices<'a> {
    fn new(exports: &'a [Export], links: &[Provision]) -> Self {
        let mut choices = Self {
            sources: exports
                .iter()
                .flat_map(|e| {
                    sources(e)
                        .into_iter()
                        .map(|s| (s.id(), (e.repository.as_str(), s)))
                })
                .collect(),
            targets: exports
                .iter()
                .flat_map(|e| {
                    e.provisioners.iter().map(|p| {
                        (
                            (p.identity.as_str(), p.node.root.as_str()),
                            (e.repository.as_str(), p),
                        )
                    })
                })
                .collect(),
            candidates: std::collections::BTreeMap::new(),
            ambiguous: std::collections::BTreeSet::new(),
        };
        for link in links {
            let Some((id, literal)) = choices.source_of(link) else {
                continue;
            };
            let Some((repository, target)) = choices
                .targets
                .get(&(link.provisioner.as_str(), link.root.as_str()))
            else {
                continue;
            };
            let (repository, site) = if target.pattern() {
                (
                    link.side
                        .split_once("::")
                        .map_or(link.side.as_str(), |(r, _)| r),
                    link.side.as_str(),
                )
            } else {
                (*repository, target.identity.as_str())
            };
            let candidate = crate::items::Candidate {
                repository: repository.into(),
                contract: literal.into(),
                site: site.into(),
            };
            if link.level == Level::Ambiguous {
                choices.ambiguous.insert(id.clone());
            }
            choices.candidates.entry(id).or_default().push(candidate);
        }
        for candidates in choices.candidates.values_mut() {
            candidates.sort_by(|a, b| {
                (&a.repository, &a.contract, &a.site).cmp(&(&b.repository, &b.contract, &b.site))
            });
            candidates.dedup_by(|a, b| a.repository == b.repository && a.contract == b.contract);
        }
        choices
    }

    fn source_of<'b>(&'b self, link: &'b Provision) -> Option<(String, &'b str)> {
        let (_, target) = self
            .targets
            .get(&(link.provisioner.as_str(), link.root.as_str()))?;
        if target.pattern() {
            Some((Source::Provisioner(target).id(), &link.contract))
        } else if link.contract.contains('*') && !target.contract.contains('*') {
            Some((
                Source::Use {
                    site: &link.side,
                    contract: &link.contract,
                }
                .id(),
                &target.contract,
            ))
        } else {
            None
        }
    }

    fn choose<'d>(
        &self,
        decision: &'d crate::decisions::Decision,
        export: &Export,
        exports: &[Export],
    ) -> std::result::Result<&'d str, crate::decisions::Cause> {
        use crate::decisions::{Cause, Kind};
        let (_, source) = self
            .sources
            .get(&decision.call)
            .filter(|(owner, _)| *owner == export.repository)
            .ok_or(Cause::CallGone)?;
        if decision.kind != Kind::AmbiguousLink || !self.ambiguous.contains(&decision.call) {
            return Err(Cause::ItemNotOpen);
        }
        let (repository, contract) = decision.target.split_once("::").ok_or(Cause::TargetGone)?;
        if self
            .candidates
            .get(&decision.call)
            .is_some_and(|candidates| {
                candidates
                    .binary_search_by(|c| {
                        (c.repository.as_str(), c.contract.as_str()).cmp(&(repository, contract))
                    })
                    .is_ok()
            })
        {
            return Ok(contract);
        }
        if !exports
            .iter()
            .filter(|e| e.repository == repository)
            .any(|e| source.target_exists(e, contract))
        {
            return Err(Cause::TargetGone);
        }
        Err(Cause::NotCandidate)
    }
}

/// Existing decision transport and stale causes also cover pattern-to-use choices.
pub(crate) fn reports(
    workspace: &crate::workspace::Workspace,
    exports: &[Export],
) -> Vec<crate::decisions::Evaluation> {
    if !exports.iter().flat_map(|e| &e.decisions.items).any(handles) {
        return exports
            .iter()
            .map(|e| crate::decisions::Evaluation {
                repository: e.repository.clone(),
                stale: Vec::new(),
                waiting: Vec::new(),
            })
            .collect();
    }
    let core = raw_links(exports);
    let choices = Choices::new(exports, &core);
    exports
        .iter()
        .map(|export| {
            let mut report = crate::decisions::Evaluation {
                repository: export.repository.clone(),
                stale: Vec::new(),
                waiting: Vec::new(),
            };
            for d in export.decisions.items.iter().filter(|d| handles(d)) {
                let source_exists = choices
                    .sources
                    .get(&d.call)
                    .is_some_and(|(owner, _)| *owner == export.repository);
                let missing = d
                    .target
                    .split_once("::")
                    .and_then(|(repo, _)| workspace.missing.iter().find(|m| m.repo == repo));
                if let Some(peer) = missing.filter(|_| source_exists) {
                    report.waiting.push(crate::decisions::Waiting {
                        call: d.call.clone(),
                        target: d.target.clone(),
                        peer: peer.repo.clone(),
                    });
                } else if let Err(cause) = choices.choose(d, export, exports) {
                    report.stale.push(crate::decisions::Stale::new(
                        d.call.clone(),
                        d.target.clone(),
                        cause,
                    ));
                }
            }
            report
        })
        .collect()
}

/// The decision transport places the patterned source, whichever direction it participates in.
pub(crate) fn item_sites(exports: &[Export]) -> Vec<(usize, String)> {
    exports
        .iter()
        .enumerate()
        .flat_map(|(n, e)| {
            uses(e)
                .filter(|(_, c)| c.contains('*'))
                .map(move |(site, _)| (n, site.to_owned()))
        })
        .collect()
}

pub(crate) fn items(
    workspace: &crate::workspace::Workspace,
    exports: &[Export],
) -> Vec<(crate::items::OpenItem, bool)> {
    use crate::items::{ItemKind, OpenItem};
    if !exports
        .iter()
        .any(|e| e.provisioners.iter().any(Side::pattern) || uses(e).any(|(_, c)| c.contains('*')))
    {
        return Vec::new();
    }
    let core = raw_links(exports);
    let choices = Choices::new(exports, &core);
    let decided = exports
        .iter()
        .flat_map(|e| {
            e.decisions
                .items
                .iter()
                .filter(|d| choices.choose(d, e, exports).is_ok())
                .map(|d| d.call.as_str())
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut items = Vec::new();
    for (export, repo) in exports.iter().zip(&workspace.repos) {
        let locations = Locations::new(export);
        let mut files = crate::render::Files::new(&repo.root);
        for (id, (_, source)) in choices.sources.iter().filter(|(id, (owner, _))| {
            *owner == export.repository && choices.ambiguous.contains(*id)
        }) {
            let (path, line, expression) = source.placement(&locations, &mut files);
            items.push((
                OpenItem {
                    id: id.clone(),
                    kind: ItemKind::Ambiguous,
                    repository: export.repository.clone(),
                    line,
                    path,
                    expression,
                    enclosing: None,
                    candidates: choices.candidates.get(id).cloned().unwrap_or_default(),
                },
                decided.contains(id.as_str()),
            ));
        }
    }
    items
}

/// A key is independently traceable; its repository-level site aggregates its config uses.
pub(crate) fn at_site(link: &Provision, site: &str) -> bool {
    link.side == site
        || (link.at.is_some()
            && link
                .side
                .split_once("::")
                .is_some_and(|(repo, _)| repo == site))
}

pub(crate) fn target<'a>(exports: &'a [Export], link: &Provision) -> Option<(usize, &'a Side)> {
    exports.iter().enumerate().find_map(|(n, e)| {
        e.provisioners
            .iter()
            .find(|p| p.identity == link.provisioner && p.node.root == link.root)
            .map(|p| (n, p))
    })
}

/// Provisioners of the symbol's own contracts and of the calls it makes.
pub(crate) fn context(exports: &[Export], symbol: &str) -> Vec<String> {
    let calls = exports
        .iter()
        .flat_map(|e| &e.calls)
        .filter(|c| c.caller == symbol)
        .map(|c| c.id.as_str())
        .collect::<Vec<_>>();
    let mut lines = links(exports)
        .iter()
        .filter(|l| l.side == symbol || calls.contains(&l.side.as_str()))
        .filter_map(|l| {
            let (n, p) = target(exports, l)?;
            Some(format!(
                "{} [{}: {}]",
                p.place(&exports[n].repository),
                l.level.name(),
                l.reason
            ))
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines.dedup();
    if lines.is_empty() {
        Vec::new()
    } else {
        crate::render::section("provisioners", lines)
    }
}
