//! Alert labels and explicit incident payloads connect monitors to native routing predicates.

use crate::contract::{ContractKind, Found, Level, Link, Related, Role};
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::resolve::Code;
use crate::workspace::Repository;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

mod config;
mod forwarding;
mod incident;
mod pcl;
mod regexp;
use config::{Matcher, Op, Route, Rule, Stream};
pub struct Alert;

// ponytail: regex source length bounds compilation input and per-label matching work.
const ALERT_REGEX_MAX: usize = 4096;
// ponytail: repetition must not expand a short expression into an unbounded automaton.
const ALERT_REGEX_SIZE_MAX: usize = 1 << 20;
// ponytail: repeated conjunction explanations must not amplify matched links without bound.
const ALERT_LINK_BYTES_MAX: usize = 4 * crate::lang::MAX_BYTES;
// ponytail: unmatched rule/tree pairs must not run without a bound just because they emit no links.
const ALERT_MATCH_WORK_MAX: usize = 1_000_000;

impl ContractKind for Alert {
    fn prefix(&self) -> &'static str {
        "alert"
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
        _: &std::path::Path,
        _: &crate::contract::ConfigFacts<'_>,
    ) -> Value {
        serde_json::to_value(config::read(path, src)).unwrap_or(Value::Null)
    }
    fn place(&self, repository: &str, site: &str) -> Option<String> {
        let (path, key) = site
            .strip_prefix(&format!("{repository}::"))?
            .split_once("::")?;
        config::is_file(path).then(|| format!("{path}#{key}"))
    }
    fn sides(&self) -> Option<(&'static str, &'static str)> {
        Some(("emitter", "matcher"))
    }
    fn starts<'a>(&self, from: &str, providers: &[(&'a str, &'a str)]) -> Vec<&'a str> {
        if split(from).is_none() {
            return Vec::new();
        }
        providers
            .iter()
            .filter_map(|(contract, site)| (*contract == from).then_some(*site))
            .collect()
    }
    fn flows_to_consumer(&self) -> bool {
        true
    }
    fn decidable(&self) -> bool {
        false
    }
    fn declare(&self, repo: &Repository, export: &mut Export) {
        let mut identities = export
            .iac
            .iter()
            .map(|n| n.identity(&repo.identity))
            .collect::<BTreeSet<_>>();
        for (path, summary) in summaries(export) {
            for rule in summary.rules {
                let node = node(
                    &path,
                    &rule.key,
                    "AlertRule",
                    &rule.name,
                    rule.line,
                    rule.end_line,
                );
                for (key, value) in rule.emissions() {
                    export.providers.push(Provider {
                        contract: contract(key, value),
                        site: node.identity(&repo.identity),
                        ..Provider::default()
                    });
                }
                if identities.insert(node.identity(&repo.identity)) {
                    export.iac.push(node);
                }
            }
            for (i, route) in summary.routes.iter().enumerate() {
                if route.barrier {
                    continue;
                }
                let node = node(
                    &path,
                    &route.key,
                    match route.stream {
                        Stream::Labels => "AlertmanagerRoute",
                        Stream::Opsgenie => "OpsgenieRoutingRule",
                        Stream::Pagerduty => "PagerdutyOrchestrationRule",
                    },
                    &route.receiver,
                    route.line,
                    route.end_line,
                );
                for matcher in conditions(&summary.routes, i) {
                    export.consumers.push(Consumer {
                        identity: None,
                        contract: Some(matcher.contract()),
                        site: node.identity(&repo.identity),
                        via: Some(matcher.via()),
                        shape: None,
                        method: None,
                        fragments: Vec::new(),
                        hops: Vec::new(),
                        root: None,
                    });
                }
                if identities.insert(node.identity(&repo.identity)) {
                    export.iac.push(node);
                }
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
    fn link(
        &self,
        consumer: &Consumer,
        _: &Repository,
        providers: &[(&Repository, &Provider)],
        exports: &[Export],
    ) -> Vec<Link> {
        let level = if consumer
            .via
            .as_deref()
            .is_some_and(|via| !matches!(via, "alert matcher =" | "alert matcher !="))
        {
            Level::Inferred
        } else {
            Level::Exact
        };
        linked(exports, providers.iter().map(|(_, p)| *p))
            .unwrap_or_default()
            .into_iter()
            .filter(|link| {
                link.consumer == consumer.site
                    && consumer.contract.as_deref() == Some(&link.contract)
                    && link.level == level
            })
            .collect()
    }
    fn workspace_links(
        &self,
        exports: &[Export],
        providers: &[(&Repository, &Provider)],
    ) -> Option<Vec<Link>> {
        Some(linked(exports, providers.iter().map(|(_, p)| *p)).unwrap_or_default())
    }
    fn workspace_related_sides<'e>(
        &self,
        exports: &'e [Export],
        links: &[Link],
        contract: &str,
    ) -> Option<Vec<Vec<Related<'e>>>> {
        let mut related = (0..exports.len())
            .map(|_| BTreeMap::new())
            .collect::<Vec<_>>();
        let key = split(contract).map(|(key, _)| key);
        let mut providers: BTreeMap<&str, Vec<(usize, &Provider)>> = BTreeMap::new();
        let mut emitters = BTreeMap::new();
        let mut exact = BTreeSet::new();
        let mut consumers = BTreeMap::new();
        for (n, export) in exports.iter().enumerate() {
            for provider in &export.providers {
                if split(&provider.contract).is_some() {
                    emitters.insert(provider.site.as_str(), (n, provider.site.as_str()));
                }
                if split(&provider.contract).map(|(key, _)| key) == key {
                    providers
                        .entry(&provider.site)
                        .or_default()
                        .push((n, provider));
                    if provider.contract == contract {
                        exact.insert(provider.site.as_str());
                    }
                }
            }
            for consumer in &export.consumers {
                if let Some(name) = &consumer.contract {
                    consumers.insert(
                        (consumer.site.as_str(), name.as_str()),
                        (n, consumer.site.as_str()),
                    );
                }
            }
        }
        for link in links {
            let mut found = Vec::new();
            if link.contract == contract {
                if let Some(providers) = providers.get(link.provider.as_str()) {
                    for &(n, provider) in providers {
                        if provider.contract != contract {
                            found.push((n, provider.site.as_str(), Role::Provider, "emitter"));
                        }
                    }
                } else if let Some(&(n, site)) = emitters.get(link.provider.as_str()) {
                    // An explicit receiver can rename a label key. Its proven link still
                    // identifies the originating monitor, without inventing an emitted label.
                    found.push((n, site, Role::Provider, "emitter"));
                }
            } else if exact.contains(link.provider.as_str())
                && let Some(&(n, site)) =
                    consumers.get(&(link.consumer.as_str(), link.contract.as_str()))
            {
                found.push((n, site, Role::Consumer, "matcher"));
            }
            for (n, site, role, words) in found {
                let value = Related {
                    site,
                    role,
                    words: words.into(),
                    matched: Some((link.level, link.reason.clone())),
                    shape: None,
                };
                let previous: &mut Related<'e> =
                    related[n].entry((role, site)).or_insert_with(|| Related {
                        site,
                        role,
                        words: words.into(),
                        matched: None,
                        shape: None,
                    });
                if previous.matched.is_none() || value.matched < previous.matched {
                    *previous = value;
                }
            }
        }
        Some(
            related
                .into_iter()
                .map(|rows| rows.into_values().collect())
                .collect(),
        )
    }
    fn side_note(&self, exports: &[Export], _: &str, role: Role) -> &'static str {
        let (emitter, matcher) = roles(exports);
        match role {
            Role::Provider if !matcher => " (no matcher in the workspace)",
            Role::Consumer if !emitter => " (no emitter in the workspace)",
            _ => "",
        }
    }
    fn scope_status(
        &self,
        _: &Repository,
        export: &Export,
        exports: &[Export],
        links: &[Link],
    ) -> Vec<String> {
        Status::new(exports, links).rows(export)
    }
    fn workspace_status(
        &self,
        _: &[Repository],
        exports: &[Export],
        links: &[Link],
    ) -> Option<Vec<Vec<String>>> {
        let status = Status::new(exports, links);
        Some(exports.iter().map(|export| status.rows(export)).collect())
    }
    fn context_at(
        &self,
        exports: &[Export],
        _: &[Repository],
        _: &[Link],
        site: &str,
        _: Option<&str>,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        for export in exports {
            for provider in export
                .providers
                .iter()
                .filter(|p| p.site == site && p.contract.starts_with("alert::"))
            {
                lines.push(format!(
                    "emitter: {}{}",
                    provider.contract,
                    self.side_note(exports, &provider.contract, Role::Provider)
                ));
            }
            for consumer in export.consumers.iter().filter(|c| {
                c.site == site
                    && c.contract
                        .as_deref()
                        .is_some_and(|c| c.starts_with("alert::"))
            }) {
                let contract = consumer.contract.as_deref().unwrap_or_default();
                lines.push(format!(
                    "matcher: {contract}{}",
                    self.side_note(exports, contract, Role::Consumer)
                ));
            }
            if let Some((routes, index)) = route_at(export, site) {
                lines.push(format!("receiver: {}", routes[index].receiver));
            }
        }
        lines
    }
}

struct Status<'a> {
    roles: (bool, bool),
    limit: Option<Limit>,
    emitted: BTreeSet<(&'a str, &'a str)>,
    received: BTreeSet<(&'a str, &'a str)>,
}
impl<'a> Status<'a> {
    fn new(exports: &[Export], links: &'a [Link]) -> Self {
        // A nonempty alert result already proves the whole pass finished. Empty results
        // need one diagnostic pass to distinguish an unmatched workspace from a limit.
        let limit = if links
            .iter()
            .any(|link| link.contract.starts_with("alert::"))
        {
            None
        } else {
            linked(exports, exports.iter().flat_map(|e| &e.providers)).err()
        };
        Self {
            roles: roles(exports),
            limit,
            emitted: links
                .iter()
                .filter_map(|link| Some((link.provider.as_str(), split(&link.contract)?.0)))
                .collect(),
            received: links
                .iter()
                .map(|link| (link.consumer.as_str(), link.contract.as_str()))
                .collect(),
        }
    }
    fn rows(&self, export: &Export) -> Vec<String> {
        let mut rows = BTreeSet::new();
        for (path, summary) in summaries(export) {
            if summary.truncated {
                rows.insert(crate::msg::Msg::AlertSummaryLimit(&path).text());
            }
        }
        if let Some(limit) = self.limit {
            if roles(std::slice::from_ref(export)) != (false, false) {
                rows.insert(match limit {
                    Limit::Output => crate::msg::Msg::AlertLinkLimit.text(),
                    Limit::Work => crate::msg::Msg::AlertMatchLimit.text(),
                });
            }
            return rows.into_iter().collect();
        }
        if self.roles != (true, true) {
            return rows.into_iter().collect();
        }
        let prefix = format!("{}::", export.repository);
        for provider in export
            .providers
            .iter()
            .filter(|p| p.contract.starts_with("alert::"))
        {
            if !split(&provider.contract)
                .is_some_and(|(key, _)| self.emitted.contains(&(provider.site.as_str(), key)))
            {
                rows.insert(
                    crate::msg::Msg::OrphanAlert {
                        contract: &provider.contract,
                        place: provider
                            .site
                            .strip_prefix(&prefix)
                            .unwrap_or(&provider.site),
                        emitter: false,
                    }
                    .text(),
                );
            }
        }
        for consumer in export.consumers.iter().filter(|c| {
            c.contract
                .as_deref()
                .is_some_and(|c| c.starts_with("alert::"))
        }) {
            let contract = consumer.contract.as_deref().unwrap_or_default();
            if !self.received.contains(&(consumer.site.as_str(), contract)) {
                rows.insert(
                    crate::msg::Msg::OrphanAlert {
                        contract,
                        place: consumer
                            .site
                            .strip_prefix(&prefix)
                            .unwrap_or(&consumer.site),
                        emitter: true,
                    }
                    .text(),
                );
            }
        }
        rows.into_iter().collect()
    }
}

#[derive(Clone, Copy)]
enum Limit {
    Output,
    Work,
}

/// Compile each routing scope once; only explicit receiver mappings cross event streams.
fn linked<'a>(
    exports: &[Export],
    providers: impl Iterator<Item = &'a Provider>,
) -> Result<Vec<Link>, Limit> {
    let inventories = exports
        .iter()
        .map(|e| (e, summaries(e)))
        .collect::<Vec<_>>();
    let rules = inventories
        .iter()
        .flat_map(|(export, configs)| {
            configs.iter().flat_map(move |(path, summary)| {
                summary
                    .rules
                    .iter()
                    .map(move |rule| (format!("{}::{path}::{}", export.repository, rule.key), rule))
            })
        })
        .collect::<Vec<_>>();
    let providers = providers
        .map(|p| (p.site.as_str(), p.contract.as_str()))
        .collect::<BTreeSet<_>>();
    let consumers = exports
        .iter()
        .flat_map(|e| &e.consumers)
        .filter_map(|c| Some((c.site.as_str(), c.contract.as_deref()?, c.via.as_deref()?)))
        .collect::<BTreeSet<_>>();
    if rules.is_empty() || providers.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut retained = 0;
    let mut work = ALERT_MATCH_WORK_MAX;
    let mut forwarded = Vec::new();
    // Alertmanager selects receivers using labels directly. Those receivers alone can
    // produce known incident fields; a matching value elsewhere is no forwarding.
    for (export, configs) in &inventories {
        for (path, summary) in configs {
            if !summary.routes.iter().any(|r| r.stream == Stream::Labels) {
                continue;
            }
            let Some(routing) = Routing::new(summary.routes.iter().collect()) else {
                continue;
            };
            let targets = summary
                .routes
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    target(
                        export,
                        path,
                        &summary.routes[i],
                        conditions(&summary.routes, i),
                        &consumers,
                    )
                })
                .collect::<Vec<_>>();
            for (site, rule) in &rules {
                if rule.stream != Stream::Labels {
                    continue;
                }
                for index in routing.destinations(rule, &mut work)? {
                    emit(
                        &targets[index],
                        &routing,
                        rule,
                        rule,
                        site,
                        None,
                        &providers,
                        &mut out,
                        &mut retained,
                        &mut work,
                    )?;
                    let at = format!(
                        "{}::{path}::{}",
                        export.repository, summary.routes[index].key
                    );
                    for event in forwarding::events(
                        summary,
                        &summary.routes[index],
                        rule,
                        &at,
                        &mut retained,
                        &mut work,
                    )? {
                        forwarded.push((site.as_str(), *rule, event));
                    }
                }
            }
        }
    }
    // Incident rules in one repository share their native scope even across Terraform files.
    for (export, configs) in &inventories {
        let mut routes = Vec::new();
        let mut targets = Vec::new();
        for (path, summary) in configs {
            for route in summary.routes.iter().filter(|r| r.stream != Stream::Labels) {
                routes.push(route);
                targets.push(target(
                    export,
                    path,
                    route,
                    route.matchers.iter().collect(),
                    &consumers,
                ));
            }
        }
        if routes.is_empty() {
            continue;
        }
        let Some(routing) = Routing::new(routes) else {
            continue;
        };
        for (site, rule) in &rules {
            if rule.stream != Stream::Opsgenie {
                continue;
            }
            for index in routing.destinations(rule, &mut work)? {
                emit(
                    &targets[index],
                    &routing,
                    rule,
                    rule,
                    site,
                    Some("forwarding: Datadog monitor tags -> OpsGenie tags"),
                    &providers,
                    &mut out,
                    &mut retained,
                    &mut work,
                )?;
            }
        }
        for (site, origin, event) in &forwarded {
            for index in routing.destinations(&event.rule, &mut work)? {
                emit(
                    &targets[index],
                    &routing,
                    &event.rule,
                    origin,
                    site,
                    Some(&event.reason),
                    &providers,
                    &mut out,
                    &mut retained,
                    &mut work,
                )?;
            }
        }
    }
    out.sort_by(|a, b| {
        (&a.consumer, &a.provider, &a.contract, a.level, &a.reason).cmp(&(
            &b.consumer,
            &b.provider,
            &b.contract,
            b.level,
            &b.reason,
        ))
    });
    out.dedup_by(|a, b| {
        a.consumer == b.consumer
            && a.provider == b.provider
            && a.contract == b.contract
            && a.level == b.level
            && a.reason == b.reason
    });
    Ok(out)
}
struct Target<'a> {
    site: String,
    why: String,
    matchers: Vec<(&'a Matcher, String, Level)>,
}
fn target<'a>(
    export: &Export,
    path: &str,
    route: &Route,
    effective: Vec<&'a Matcher>,
    consumers: &BTreeSet<(&str, &str, &str)>,
) -> Target<'a> {
    let site = format!("{}::{path}::{}", export.repository, route.key);
    let why = format!(
        "matchers {}; receiver: {}",
        effective
            .iter()
            .map(|m| m.expression())
            .collect::<Vec<_>>()
            .join(", "),
        route.receiver
    );
    let mut seen = BTreeSet::new();
    let matchers = effective
        .into_iter()
        .filter_map(|m| {
            let contract = m.contract();
            let via = m.via();
            if !consumers.contains(&(site.as_str(), contract.as_str(), via.as_str()))
                || !seen.insert((contract.clone(), via))
            {
                return None;
            }
            Some((
                m,
                contract,
                if m.op.inferred() {
                    Level::Inferred
                } else {
                    Level::Exact
                },
            ))
        })
        .collect();
    Target {
        site,
        why,
        matchers,
    }
}
#[expect(
    clippy::too_many_arguments,
    reason = "one bounded workspace pass shares its output and work budgets"
)]
fn emit(
    target: &Target<'_>,
    routing: &Routing<'_>,
    event: &Rule,
    origin: &Rule,
    site: &str,
    forwarding: Option<&str>,
    providers: &BTreeSet<(&str, &str)>,
    out: &mut Vec<Link>,
    retained: &mut usize,
    work: &mut usize,
) -> Result<(), Limit> {
    for (matcher, name, level) in &target.matchers {
        *work = work.checked_sub(1).ok_or(Limit::Work)?;
        if routing.condition(matcher).matches(event) != Some(true) {
            continue;
        }
        let emitted = origin.emissions().any(|(key, value)| {
            (forwarding.is_some() || key == matcher.key || (matcher.tags && matcher.key == "tags"))
                && providers.contains(&(site, contract(key, value).as_str()))
        });
        if !emitted {
            continue;
        }
        let extra = forwarding.map_or(0, |f| f.len() + 2);
        for bytes in [
            target.site.len(),
            site.len(),
            name.len(),
            name.len(),
            2,
            target.why.len(),
            extra,
            std::mem::size_of::<Link>(),
        ] {
            forwarding::reserve(retained, bytes)?;
        }
        let mut reason = format!("{name}; {}", target.why);
        if let Some(forwarding) = forwarding {
            reason.push_str("; ");
            reason.push_str(forwarding);
        }
        out.push(Link::new(
            target.site.clone(),
            site.into(),
            name.clone(),
            *level,
            reason,
            None,
            None,
            None,
        ));
    }
    Ok(())
}

fn roles(exports: &[Export]) -> (bool, bool) {
    (
        exports.iter().any(|e| {
            e.providers
                .iter()
                .any(|p| p.contract.starts_with("alert::"))
        }),
        exports.iter().any(|e| {
            e.consumers.iter().any(|c| {
                c.contract
                    .as_deref()
                    .is_some_and(|c| c.starts_with("alert::"))
            })
        }),
    )
}
fn contract(key: &str, value: &str) -> String {
    format!("alert::{key}::{value}")
}
fn split(contract: &str) -> Option<(&str, &str)> {
    contract.strip_prefix("alert::")?.split_once("::")
}
fn node(
    path: &str,
    key: &str,
    ty: &str,
    name: &str,
    line: usize,
    end_line: usize,
) -> crate::iac::Node {
    crate::iac::Node {
        address: key.into(),
        root: path.into(),
        ty: ty.into(),
        name: Some(name.into()),
        path: path.into(),
        line,
        end_line,
        block: key.into(),
        ..Default::default()
    }
}
fn summaries(export: &Export) -> Vec<(String, config::Summary)> {
    export
        .configurations
        .iter()
        .filter_map(|(path, value)| {
            Some((
                path.clone(),
                serde_json::from_value(value.get("alert")?.clone()).ok()?,
            ))
        })
        .collect()
}
fn route_at(export: &Export, site: &str) -> Option<(Vec<Route>, usize)> {
    summaries(export).into_iter().find_map(|(path, s)| {
        let index = s.routes.iter().position(|r| {
            !r.barrier && site == format!("{}::{path}::{}", export.repository, r.key)
        })?;
        Some((s.routes, index))
    })
}
fn conditions(routes: &[Route], mut index: usize) -> Vec<&Matcher> {
    let mut result = Vec::new();
    loop {
        result.extend(routes[index].matchers.iter());
        let Some(parent) = routes[index].parent else {
            break;
        };
        index = parent;
    }
    result
}

struct Condition<'a> {
    matcher: &'a Matcher,
    regex: Option<std::sync::Arc<regex::Regex>>,
}
impl Condition<'_> {
    fn positive(&self, value: &str) -> Option<bool> {
        let matcher = self.matcher;
        let expected = matcher.pattern();
        Some(match matcher.op {
            Op::Regex | Op::NotRegex => self.regex.as_ref()?.is_match(value),
            Op::Equal | Op::NotEqual => value == expected,
            Op::Prefix | Op::NotPrefix => value.starts_with(expected),
            Op::Contains | Op::NotContains => value.contains(expected),
            Op::Suffix | Op::NotSuffix => value.ends_with(expected),
        })
    }
    fn matches(&self, rule: &Rule) -> Option<bool> {
        let matcher = self.matcher;
        if matcher.op.regex() && self.regex.is_none() {
            return None;
        }
        if matcher.tags {
            if rule.stream != Stream::Opsgenie {
                return None;
            }
            let mut unknown = rule.unknown_tags;
            for tag in &rule.tags {
                match self.positive(tag) {
                    Some(true) => return Some(!matcher.op.negative()),
                    Some(false) => {}
                    None => unknown = true,
                }
            }
            return (!unknown).then_some(matcher.op.negative());
        }
        let field = if matcher.field.is_empty() {
            &matcher.key
        } else {
            &matcher.field
        };
        if rule.unknown.contains(field) {
            return None;
        }
        // Unwritten expression-result labels/event fields are unknown, not empty strings.
        let value = rule.labels.get(field)?;
        Some(self.positive(value)? != matcher.op.negative())
    }
}
struct Routing<'a> {
    routes: Vec<&'a Route>,
    conditions: Vec<Vec<Condition<'a>>>,
    compiled: BTreeMap<&'a str, Option<std::sync::Arc<regex::Regex>>>,
    children: Vec<Vec<usize>>,
    roots: Vec<Vec<usize>>,
}
impl<'a> Routing<'a> {
    fn new(routes: Vec<&'a Route>) -> Option<Self> {
        let patterns = routes
            .iter()
            .flat_map(|r| &r.matchers)
            .filter(|m| m.op.regex())
            .map(|m| m.pattern())
            .collect::<BTreeSet<_>>();
        let budget = ALERT_REGEX_SIZE_MAX / patterns.len().max(1);
        let compiled = patterns
            .into_iter()
            .map(|p| (p, regexp::compile(p, budget).map(std::sync::Arc::new)))
            .collect::<BTreeMap<_, _>>();
        let mut conditions = Vec::new();
        let mut children = vec![Vec::new(); routes.len()];
        let mut roots = Vec::new();
        let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
        let mut sets: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
        let unknown_scopes = routes
            .iter()
            .filter(|r| r.stream == Stream::Opsgenie && r.unknown_scope)
            .map(|r| r.group.as_str())
            .collect::<BTreeSet<_>>();
        for (i, route) in routes.iter().enumerate() {
            if route.stream == Stream::Pagerduty {
                sets.entry((&route.group, &route.set)).or_default().push(i);
                if route.set == "start" {
                    groups
                        .entry((route.group.clone(), "pagerduty".into()))
                        .or_default()
                        .push(i);
                }
            } else if let Some(parent) = route.parent {
                if parent >= i {
                    return None;
                }
                children.get_mut(parent)?.push(i);
            } else if route.stream == Stream::Opsgenie {
                groups
                    .entry((route.group.clone(), route.receiver.clone()))
                    .or_default()
                    .push(i);
            } else {
                roots.push(vec![i]);
            }
            conditions.push(
                route
                    .matchers
                    .iter()
                    .map(|matcher| Condition {
                        matcher,
                        regex: compiled.get(matcher.pattern()).cloned().flatten(),
                    })
                    .collect(),
            );
        }
        for (i, route) in routes.iter().enumerate() {
            if route.stream == Stream::Pagerduty
                && let Some(next) = &route.next
            {
                children[i] = sets
                    .get(&(route.group.as_str(), next.as_str()))
                    .cloned()
                    .unwrap_or_default();
            }
        }
        for mut group in groups.into_values() {
            // An unread team expression might name any scope in this module. Its absence
            // from another team's ordered list cannot prove that list receives the event.
            if routes[group[0]].stream == Stream::Opsgenie
                && unknown_scopes.contains(routes[group[0]].group.as_str())
            {
                continue;
            }
            if group.len() > 1 && routes[group[0]].stream == Stream::Opsgenie {
                let order = group
                    .iter()
                    .filter_map(|&i| routes[i].order)
                    .collect::<BTreeSet<_>>();
                // Without a complete, unique native order no first match is certain.
                if order.len() != group.len() {
                    continue;
                }
            }
            group.sort_by_key(|&i| routes[i].order.unwrap_or(0));
            roots.push(group);
        }
        Some(Self {
            routes,
            conditions,
            compiled,
            children,
            roots,
        })
    }
    fn condition(&self, matcher: &'a Matcher) -> Condition<'a> {
        Condition {
            matcher,
            regex: self.compiled.get(matcher.pattern()).cloned().flatten(),
        }
    }
    fn matches(&self, index: usize, rule: &Rule, work: &mut usize) -> Result<Option<bool>, Limit> {
        *work = work.checked_sub(1).ok_or(Limit::Work)?;
        let route = self.routes[index];
        if rule.stream != route.stream {
            return Ok(Some(false));
        }
        if !rule.scopes.is_empty()
            && route.stream == Stream::Opsgenie
            && !rule.scopes.contains(&route.receiver)
        {
            return Ok(Some(false));
        }
        if !route.valid {
            return Ok(None);
        }
        let mut known = !route.opaque;
        let mut values = Vec::new();
        for condition in &self.conditions[index] {
            *work = work.checked_sub(1).ok_or(Limit::Work)?;
            let value = condition.matches(rule);
            if !route.logic.is_empty() {
                values.push(value);
                continue;
            }
            match value {
                Some(false) if !route.any => return Ok(Some(false)),
                Some(true) if route.any => return Ok(Some(true)),
                Some(_) => {}
                None => known = false,
            }
        }
        if !route.logic.is_empty() {
            return Ok(pcl::evaluate(&route.logic, &values));
        }
        Ok(known.then_some(!route.any))
    }
    fn siblings(
        &self,
        siblings: &[usize],
        rule: &Rule,
        work: &mut usize,
    ) -> Result<(Vec<usize>, bool), Limit> {
        let mut selected = Vec::new();
        let mut unknown = false;
        for &index in siblings {
            match self.matches(index, rule, work)? {
                Some(false) => continue,
                None => unknown = true,
                Some(true) => selected.push(index),
            }
            if !self.routes[index].continuing {
                break;
            }
        }
        Ok((selected, unknown))
    }
    fn destinations(&self, rule: &Rule, work: &mut usize) -> Result<BTreeSet<usize>, Limit> {
        let mut out = BTreeSet::new();
        let mut pending = Vec::new();
        for roots in &self.roots {
            pending.extend(self.siblings(roots, rule, work)?.0);
        }
        let mut visited = BTreeSet::new();
        while let Some(index) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            let (selected, unknown) = self.siblings(&self.children[index], rule, work)?;
            if self.routes[index].stream == Stream::Pagerduty || (selected.is_empty() && !unknown) {
                out.insert(index);
            }
            pending.extend(selected);
        }
        Ok(out)
    }
}

#[cfg(test)]
fn regex(pattern: &str) -> Option<regex::Regex> {
    regexp::compile(pattern, ALERT_REGEX_SIZE_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonselected_incident_receivers_use_the_shared_evaluation_ceiling() {
        let rules = (0..1000)
            .map(|i| serde_json::json!({"alert":format!("R{i}"), "expr":"up"}))
            .collect::<Vec<_>>();
        let receivers = std::iter::once(serde_json::json!({"name":"selected"})).chain((0..1000).map(|i| serde_json::json!({"name":format!("unused{i}"), "opsgenie_configs":[{"tags":"support:team-b"}]}))).collect::<Vec<_>>();
        let source = serde_json::json!({"groups":[{"name":"many", "rules":rules}], "route":{"receiver":"selected"}, "receivers":receivers}).to_string();
        assert!(source.len() < crate::lang::MAX_BYTES);
        let repo = Repository {
            identity: "example.com/forwarding-work".into(),
            root: Default::default(),
            config: Default::default(),
            decisions: Default::default(),
            index: Default::default(),
        };
        let mut export = Export {
            repository: repo.identity.clone(),
            configurations: vec![(
                "alert.yaml".into(),
                serde_json::json!({"alert":config::read("alert.yaml", source.as_bytes())}),
            )],
            ..Default::default()
        };
        Alert.declare(&repo, &mut export);
        let exports = [export];
        let providers = exports[0]
            .providers
            .iter()
            .map(|p| (&repo, p))
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let links = Alert.workspace_links(&exports, &providers).unwrap();
        assert!(links.is_empty());
        assert_eq!(
            Alert.scope_status(&repo, &exports[0], &exports, &links),
            ["alert routing skipped: evaluation limit"]
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn receiver_template_expansion_uses_the_shared_output_ceiling_without_partial_links() {
        let value = "b".repeat(4096);
        let copies = ALERT_LINK_BYTES_MAX / value.len() + 1;
        let source = serde_json::json!({
            "groups":[{"name":"wide", "rules":[{"alert":"Wide", "expr":"up", "labels":{"owner":value}}]}],
            "route":{"receiver":"incident", "group_by":["owner"]},
            "receivers":[{"name":"incident", "opsgenie_configs":[{"tags":"target:{{ .CommonLabels.owner }},".repeat(copies)}]}]
        }).to_string();
        assert!(source.len() < crate::lang::MAX_BYTES);
        let repo = Repository {
            identity: "example.com/forwarding-limit".into(),
            root: Default::default(),
            config: Default::default(),
            decisions: Default::default(),
            index: Default::default(),
        };
        let mut export = Export {
            repository: repo.identity.clone(),
            configurations: vec![(
                "alert.yaml".into(),
                serde_json::json!({"alert":config::read("alert.yaml", source.as_bytes())}),
            )],
            ..Default::default()
        };
        Alert.declare(&repo, &mut export);
        let exports = [export];
        let providers = exports[0]
            .providers
            .iter()
            .map(|p| (&repo, p))
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let links = Alert.workspace_links(&exports, &providers).unwrap();
        assert!(links.is_empty(), "{} links", links.len());
        assert_eq!(
            Alert.scope_status(&repo, &exports[0], &exports, &links),
            ["alert routing skipped: link output limit"]
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn wide_matched_conjunctions_stop_at_the_link_output_ceiling_without_partial_links() {
        let labels = (0..1000)
            .map(|i| (format!("key{i:04}"), Value::String("team-b".into())))
            .collect::<serde_json::Map<_, _>>();
        let source = serde_json::json!({
            "groups":[{"name":"wide", "rules":[{"alert":"Wide", "expr":"up", "labels":labels}]}],
            "route":{"receiver":"wide", "match":labels}, "receivers":[{"name":"wide"}]
        })
        .to_string();
        assert!(source.len() < crate::lang::MAX_BYTES);
        assert!(1000 * source.len() > ALERT_LINK_BYTES_MAX);
        let repo = Repository {
            identity: "example.com/bounded".into(),
            root: Default::default(),
            config: Default::default(),
            decisions: Default::default(),
            index: Default::default(),
        };
        let mut export = Export {
            repository: repo.identity.clone(),
            configurations: vec![(
                "alert.yaml".into(),
                serde_json::json!({"alert":config::read("alert.yaml", source.as_bytes())}),
            )],
            ..Default::default()
        };
        Alert.declare(&repo, &mut export);
        let exports = [export];
        let providers = exports[0]
            .providers
            .iter()
            .map(|p| (&repo, p))
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let links = Alert.workspace_links(&exports, &providers).unwrap();
        assert!(links.is_empty(), "{} links", links.len());
        let status = Alert.scope_status(&repo, &exports[0], &exports, &links);
        assert_eq!(status, ["alert routing skipped: link output limit"]);
        eprintln!("wide conjunction output limit: {:?}", started.elapsed());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn unmatched_rule_route_pairs_stop_at_the_evaluation_ceiling() {
        let rules = (0..1000).map(|i| serde_json::json!({"alert":format!("R{i}"), "expr":"up", "labels":{"support":"other"}})).collect::<Vec<_>>();
        let routes = (0..600).map(|_| serde_json::json!({"receiver":"team", "match":{"support":"team-b"}, "continue":true})).collect::<Vec<_>>();
        assert!(rules.len() * routes.len() * 2 > ALERT_MATCH_WORK_MAX);
        let source = serde_json::json!({"groups":[{"name":"wide", "rules":rules}], "route":{"receiver":"team", "routes":routes}, "receivers":[{"name":"team"}]}).to_string();
        assert!(source.len() < crate::lang::MAX_BYTES);
        let repo = Repository {
            identity: "example.com/work".into(),
            root: Default::default(),
            config: Default::default(),
            decisions: Default::default(),
            index: Default::default(),
        };
        let mut export = Export {
            repository: repo.identity.clone(),
            configurations: vec![(
                "alert.yaml".into(),
                serde_json::json!({"alert":config::read("alert.yaml", source.as_bytes())}),
            )],
            ..Default::default()
        };
        Alert.declare(&repo, &mut export);
        let exports = [export];
        let providers = exports[0]
            .providers
            .iter()
            .map(|p| (&repo, p))
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let links = Alert.workspace_links(&exports, &providers).unwrap();
        assert!(links.is_empty());
        let status = Alert.scope_status(&repo, &exports[0], &exports, &links);
        assert_eq!(status, ["alert routing skipped: evaluation limit"]);
        eprintln!(
            "unmatched routing evaluation limit: {:?}",
            started.elapsed()
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(30));
    }

    #[test]
    fn regex_source_and_automaton_ceilings_leave_large_matchers_unmatched() {
        assert!(regex(&"a".repeat(ALERT_REGEX_MAX)).is_some());
        assert!(regex(&"a".repeat(ALERT_REGEX_MAX + 1)).is_none());
        assert!(regex("a{100000000}").is_none());
        assert!(regex("[").is_none());
    }
}

#[cfg(test)]
mod regexp_semantics {
    use super::*;

    #[test]
    fn re2_counted_repeats_and_unicode_properties_do_not_use_rust_only_syntax() {
        for pattern in [
            "(?P<a.b>team-b)",
            "(?P<é>team-b)",
            "(?<a.b>team-b)",
            "(?<é>team-b)",
            r"(?P<\Qname\E>team-b)",
            r"(?<\Qname\E>team-b)",
        ] {
            assert!(regex(pattern).is_none(), "{pattern}");
        }
        for pattern in [
            r"(?P<team_1>team-b)",
            r"(?<team_1>team-b)",
            r"(?P<team_1>\Qteam-b\E)",
            r"(?<team_1>\Qteam-b\E)",
        ] {
            assert!(regex(pattern).unwrap().is_match("team-b"), "{pattern}");
        }
        assert!(regex("a{01}").is_none());
        assert!(regex("a{ 1}").is_none());
        assert!(regex("a{\u{a0}1}").is_none());
        assert!(regex(r"a\{01\}").unwrap().is_match("a{01}"));
        assert!(regex("a{1001}").is_none());
        assert!(regex("(?:a{1000}){2}").is_none());
        assert!(regex("a{1000}").is_some());
        assert!(regex(r"\p{Alphabetic}").is_none());
        assert!(regex(r"\p{Latn}").is_none());
        assert!(regex(r"\p{Latin}").unwrap().is_match("A"));
        assert!(regex(r"\pL").unwrap().is_match("é"));
        assert!(regex(r"\123").unwrap().is_match("S"));
        assert!(regex(r"\1").is_none());
        assert!(!regex(".").unwrap().is_match("\n"));
        assert!(regex("(?s:.)").unwrap().is_match("\n"));
    }

    #[test]
    fn alertmanager_perl_classes_are_ascii_and_quoted_sections_are_literal() {
        assert!(regex(r"[\Qab\E]").is_none());
        let digits = regex(r"\d+").unwrap();
        assert!(digits.is_match("123"));
        assert!(!digits.is_match("١٢٣"));
        assert!(regex(r"\Qteam.[a]\E").unwrap().is_match("team.[a]"));
        assert!(regex("(?x:team - b)").is_none());
        assert!(regex("(?u:team-b)").is_none());
        let nested = regex("[a[b]]").unwrap();
        assert!(nested.is_match("a]"));
        assert!(!nested.is_match("a"));
        let initial = regex("[^]&&a]").unwrap();
        assert!(!initial.is_match("a"));
        assert!(initial.is_match("b"));
        assert!(regex("[]&&a]").unwrap().is_match("&"));
        assert!(regex("[a--b]").is_none());
    }
}
