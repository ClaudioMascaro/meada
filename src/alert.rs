//! Alert labels and explicit incident payloads connect monitors to native routing predicates.

use crate::contract::{ContractKind, Found, Level, Link, Related, Role};
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::resolve::Code;
use crate::workspace::Repository;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

mod config;
mod forwarding;
mod incident;
mod pcl;
mod regexp;
use config::{Matcher, Op, Route, Rule, Stream};
pub struct Alert;

/// Why Alertmanager refuses to load a configuration the file at `path` holds, if it does: a
/// matcher holds a regular expression Go cannot compile. `meada status` counts such a file as
/// unreadable (`config_values`), and it gives no alert side or link.
pub(crate) fn refused(path: &str, documents: &[Value]) -> Option<String> {
    config::is_file(path)
        .then(|| documents.iter().find_map(config::refused))
        .flatten()
}

/// Whether the alert reader takes a rule, a route or a receiver from the configuration file at
/// `path` with bytes `src`: then the file keeps every config value (`config_values::kind_file`).
pub(crate) fn takes(path: &str, src: &[u8]) -> bool {
    let summary = config::read(path, src);
    !(summary.rules.is_empty() && summary.routes.is_empty() && summary.forwardings.is_empty())
}

/// The documents of a configuration file that the alert reader takes a rule, a route or a
/// receiver from, each once, read from the file's stored summaries (`contract_configs`): `None`
/// for the only document of a file, else the index of each such document of a stream (ADR 0026,
/// "Orphan sides").
pub(crate) fn documents(summaries: &Value) -> Vec<Option<usize>> {
    let Some(summary) = (summaries.get("alert").cloned())
        .and_then(|s| serde_json::from_value::<config::Summary>(s).ok())
    else {
        return Vec::new();
    };
    let rules = (summary.rules.iter()).map(|r| crate::config_values::leading_index(&r.key));
    let routes = (summary.routes.iter()).map(|r| Some(r.document));
    let receivers = (summary.forwardings.iter()).map(|f| Some(f.document));
    let taken = rules.chain(routes).chain(receivers);
    let mut documents: Vec<_> = match summary.stream {
        true => taken.collect(),
        false => taken.take(1).map(|_| None).collect(),
    };
    documents.sort();
    documents.dedup();
    documents
}

// ponytail: regex source length bounds compilation input; `ALERT_SCAN_MAX` bounds matching.
const ALERT_REGEX_MAX: usize = 4096;
/// The bytes the automata and search caches of one routing scope may take, shared equally by
/// its distinct expressions.
// ponytail: repetition must not expand a short expression into an unbounded automaton.
const ALERT_REGEX_SIZE_MAX: usize = 1 << 20;
/// The bytes the automata of one linking pass may take, in equal parts per routing scope that
/// compiles an expression, each part at most `ALERT_REGEX_SIZE_MAX`.
// ponytail: 400 route configurations compiled a megabyte each in 14.6 s (debug); 16 MiB holds
// about 10,000 ordinary expressions of 1.5 KiB.
const ALERT_REGEX_POOL_MAX: usize = 16 << 20;
/// The code points case folding may visit for the expressions of one linking pass, in equal
/// parts per routing scope and, within a scope, per distinct expression: `regex-syntax` folds a
/// case-insensitive class by visiting every code point of its ranges that hold a cased one,
/// before any automaton is built.
// ponytail: 2^22 folds a thousand `(?i)\p{L}` (3,739 code points in cased ranges each) and stops
// a pattern of hundreds of `(?i)\p{Any}` (1.1 million each), which took minutes to compile.
const ALERT_REGEX_FOLD_MAX: usize = 1 << 22;
/// The Unicode classes the expressions of one linking pass may hold, in the same equal parts:
/// `regex-syntax` looks each up in its tables and joins it to the bracket around it, before any
/// automaton is built.
// ponytail: 4096 classes read in a fraction of a second, where one route configuration of 240
// expressions of 1,358 `\pL` each took 55 s.
const ALERT_REGEX_CLASSES_MAX: usize = 4096;
// ponytail: repeated conjunction explanations must not amplify matched links without bound.
const ALERT_LINK_BYTES_MAX: usize = 4 * crate::lang::MAX_BYTES;
// ponytail: unmatched rule/tree pairs must not run without a bound just because they emit no links.
const ALERT_MATCH_WORK_MAX: usize = 1_000_000;
/// The steps matching may take in one workspace pass: a condition is charged, before it reads a
/// value, what matching it may take, `regexp::Compiled::cost` for an expression and the bytes it
/// reads for a comparison or a substring search.
// ponytail: `.*a.{1000}a.*` ran 84 ns a step (debug) on a label of 60,000 bytes mixing `a` and
// `b`, where four such routes took 20.5 s; 2^26 steps take about 5.6 s at that rate, 16.5 s for
// a class of many single characters (243 ns a step).
const ALERT_SCAN_MAX: usize = 1 << 26;

/// What one workspace pass may still spend: route visits, matcher evaluations and template bytes
/// within `ALERT_MATCH_WORK_MAX`, matching steps within `ALERT_SCAN_MAX`.
struct Work {
    units: usize,
    steps: usize,
}
impl Work {
    fn spend(&mut self, units: usize) -> Result<(), Limit> {
        self.units = self.units.checked_sub(units).ok_or(Limit::Work)?;
        Ok(())
    }
    fn scan(&mut self, steps: usize) -> Result<(), Limit> {
        self.steps = self.steps.checked_sub(steps).ok_or(Limit::Work)?;
        Ok(())
    }
}

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
                // A negated condition only filters the route: the value it names is refused.
                for matcher in conditions(&summary.routes, i)
                    .into_iter()
                    .filter(|m| !m.op.negative())
                {
                    export.consumers.push(Consumer {
                        identity: None,
                        contract: Some(matcher.contract()),
                        site: node.identity(&repo.identity),
                        via: Some(route.via(matcher)),
                        shape: None,
                        method: None,
                        fragments: Vec::new(),
                        hops: Vec::new(),
                        root: None,
                        lead: None,
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
    /// Never called: `workspace_links` derives every alert link at once.
    fn link(
        &self,
        _: &Consumer,
        _: &Repository,
        _: &[(&Repository, &Provider)],
        _: &[Export],
    ) -> Vec<Link> {
        Vec::new()
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
    let mut work = Work {
        units: ALERT_MATCH_WORK_MAX,
        steps: ALERT_SCAN_MAX,
    };
    let scopes = (inventories.iter())
        .map(|(_, configs)| regex_scopes(configs))
        .sum();
    let mut pass = regexp::Pass::new(scopes);
    let mut forwarded = Vec::new();
    // Alertmanager selects receivers using labels directly. Those receivers alone can
    // produce known incident fields; a matching value elsewhere is no forwarding.
    for (export, configs) in &inventories {
        for (path, summary) in configs {
            if !summary.routes.iter().any(|r| r.stream == Stream::Labels) {
                continue;
            }
            let Some(routing) = Routing::new(summary.routes.iter().collect(), &mut pass) else {
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
        let Some(routing) = Routing::new(routes, &mut pass) else {
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
/// The routing scopes of one repository's route configurations that compile an expression, as
/// `linked` forms them: each configuration with a label route (all its routes together), and
/// the incident routes of all of them.
fn regex_scopes(configs: &[(String, config::Summary)]) -> usize {
    let regex = |route: &Route| route.matchers.iter().any(|m| m.op.regex());
    let labels = (configs.iter())
        .filter(|(_, summary)| {
            summary.routes.iter().any(|r| r.stream == Stream::Labels)
                && summary.routes.iter().any(regex)
        })
        .count();
    let incident = (configs.iter())
        .flat_map(|(_, summary)| &summary.routes)
        .any(|r| r.stream != Stream::Labels && regex(r));
    labels + usize::from(incident)
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
            let via = route.via(m);
            if !consumers.contains(&(site.as_str(), contract.as_str(), via.as_str()))
                || !seen.insert((contract.clone(), via))
            {
                return None;
            }
            Some((
                m,
                contract,
                if m.op.inferred() || route.namespaced {
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
    work: &mut Work,
) -> Result<(), Limit> {
    for (matcher, name, level) in &target.matchers {
        work.spend(1)?;
        if routing.condition(matcher).matches(event, work)? != Some(true) {
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
    regex: Option<Arc<regexp::Compiled>>,
}
impl Condition<'_> {
    fn positive(&self, value: &str) -> Option<bool> {
        let matcher = self.matcher;
        let expected = matcher.pattern();
        Some(match matcher.op {
            Op::Regex | Op::NotRegex => self.regex.as_ref()?.regex.is_match(value),
            Op::Equal | Op::NotEqual => value == expected,
            Op::Prefix | Op::NotPrefix => value.starts_with(expected),
            Op::Contains | Op::NotContains => value.contains(expected),
            Op::Suffix | Op::NotSuffix => value.ends_with(expected),
        })
    }
    /// An upper bound on the steps matching `value` takes: its expression's, or the bytes a
    /// comparison or a substring search reads.
    fn cost(&self, value: &str) -> usize {
        match (self.matcher.op, &self.regex) {
            (Op::Regex | Op::NotRegex, Some(compiled)) => compiled.cost(value.len()),
            (Op::Contains | Op::NotContains, _) => value.len(),
            _ => value.len().min(self.matcher.pattern().len()),
        }
    }
    /// Whether `rule` meets the condition, `None` when unknown, charging `work` the steps matching
    /// the values it reads may take, before reading them.
    fn matches(&self, rule: &Rule, work: &mut Work) -> Result<Option<bool>, Limit> {
        let matcher = self.matcher;
        if matcher.op.regex() && self.regex.is_none() {
            return Ok(None);
        }
        if matcher.tags {
            if rule.stream != Stream::Opsgenie {
                return Ok(None);
            }
            let cost =
                (rule.tags.iter()).fold(0, |cost: usize, tag| cost.saturating_add(self.cost(tag)));
            work.scan(cost)?;
            let mut unknown = rule.unknown_tags;
            for tag in &rule.tags {
                match self.positive(tag) {
                    Some(true) => return Ok(Some(!matcher.op.negative())),
                    Some(false) => {}
                    None => unknown = true,
                }
            }
            return Ok((!unknown).then_some(matcher.op.negative()));
        }
        let field = if matcher.field.is_empty() {
            &matcher.key
        } else {
            &matcher.field
        };
        if rule.unknown.contains(field) {
            return Ok(None);
        }
        // Unwritten expression-result labels/event fields are unknown, not empty strings.
        let Some(value) = rule.labels.get(field) else {
            return Ok(None);
        };
        work.scan(self.cost(value))?;
        Ok(self
            .positive(value)
            .map(|positive| positive != matcher.op.negative()))
    }
}
struct Routing<'a> {
    routes: Vec<&'a Route>,
    conditions: Vec<Vec<Condition<'a>>>,
    compiled: BTreeMap<&'a str, Option<Arc<regexp::Compiled>>>,
    children: Vec<Vec<usize>>,
    roots: Vec<Vec<usize>>,
}
impl<'a> Routing<'a> {
    fn new(routes: Vec<&'a Route>, pass: &mut regexp::Pass<'a>) -> Option<Self> {
        let patterns = routes
            .iter()
            .flat_map(|r| &r.matchers)
            .filter(|m| m.op.regex())
            .map(|m| m.pattern())
            .collect::<BTreeSet<_>>();
        let shares = patterns.len();
        let compiled = patterns
            .into_iter()
            .map(|p| (p, pass.compile(p, shares)))
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
    fn matches(&self, index: usize, rule: &Rule, work: &mut Work) -> Result<Option<bool>, Limit> {
        work.spend(1)?;
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
            work.spend(1)?;
            let value = condition.matches(rule, work)?;
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
        work: &mut Work,
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
    fn destinations(&self, rule: &Rule, work: &mut Work) -> Result<BTreeSet<usize>, Limit> {
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
    (regexp::Pass::new(1).compile(pattern, 1)).map(|compiled| compiled.regex.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documents the alert reader takes a rule, a route or a receiver from (#800): the only
    /// document of a file is `None`, a document of a stream its index, a receiver's too.
    #[test]
    fn documents_are_those_a_rule_a_route_or_a_receiver_comes_from() {
        let documents = |path: &str, src: &str| {
            let summary = serde_json::to_value(config::read(path, src.as_bytes())).unwrap();
            super::documents(&serde_json::json!({ "alert": summary }))
        };
        let none = Vec::<Option<usize>>::new();
        let rule = "groups:\n- name: g\n  rules:\n  - alert: Down\n    expr: up == 0\n";
        let receiver = "receivers:\n- name: pd\n  pagerduty_configs:\n  - routing_key: k\n";
        let route = "route:\n  receiver: team\nreceivers:\n- name: team\n";
        let operator = "apiVersion: monitoring.coreos.com/v1alpha1\nkind: AlertmanagerConfig\n\
                        metadata:\n  name: jobs\nspec:\n  route:\n    receiver: jobs\n  \
                        receivers:\n  - name: jobs\n";
        assert_eq!(documents("rules.yaml", rule), [None]);
        assert_eq!(documents("pd.yaml", receiver), [None]);
        assert_eq!(documents("alertmanager.yml", route), [None]);
        assert_eq!(documents("values.yaml", "queue: jobs\n"), none);
        assert_eq!(
            documents(
                "stream.yaml",
                &format!("{receiver}---\nqueue: jobs\n---\n{rule}")
            ),
            [Some(0), Some(2)]
        );
        assert_eq!(
            documents("k8s.yaml", &format!("kind: ConfigMap\n---\n{operator}")),
            [Some(1)]
        );
        // A file past the reader's budget keeps nothing, so no document of it is the reader's.
        let wide = serde_json::json!({"route": {"receiver": "x".repeat(400_000),
            "routes": vec![serde_json::json!({}); 50_000]}, "receivers": []});
        assert_eq!(documents("alerts.json", &wide.to_string()), none);
    }

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

    /// Matching steps have their own ceiling: 1,000 rules tried against 400 continuing routes of
    /// `.*-apix`, then linked by `other-.*`, spend about 805,000 visits and evaluations, as they
    /// did before steps were charged, and 33.6 million steps (7 positions, the loop of `.*`
    /// included, at each of the 12 offsets of an 11-byte label), below `ALERT_SCAN_MAX`, so every
    /// rule links. Charged in units of 64 steps against the evaluation ceiling, those 84 steps
    /// were one unit a pair, past it.
    #[test]
    fn matching_steps_leave_the_evaluation_ceiling_to_visits_and_evaluations() {
        let rules = (0..1000).map(|i| serde_json::json!({"alert":format!("R{i}"), "expr":"up", "labels":{"support":"other-thing"}})).collect::<Vec<_>>();
        let mut routes = (0..400).map(|_| serde_json::json!({"receiver":"team", "match_re":{"support":".*-apix"}, "continue":true})).collect::<Vec<_>>();
        assert!(rules.len() * (1 + routes.len() * 2) < ALERT_MATCH_WORK_MAX);
        assert!(rules.len() * routes.len() * 3 > ALERT_MATCH_WORK_MAX);
        let looped = regexp::Pass::new(1).compile(".*-apix", 1).unwrap();
        assert_eq!(looped.cost("other-thing".len()), 84);
        routes.push(serde_json::json!({"receiver":"team", "match_re":{"support":"other-.*"}}));
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
        let links = Alert.workspace_links(&exports, &providers).unwrap();
        let status = Alert.scope_status(&repo, &exports[0], &exports, &links);
        assert_eq!(links.len(), rules.len());
        assert!(
            !status.iter().any(|row| row.contains("limit")),
            "{status:?}"
        );
    }

    /// A route expression is charged, before it runs, a step per automaton position and offset
    /// of the value it may start at: `.*a.{1000}a.*` may run its 1,003 positions at each offset
    /// of a label of 300,000 bytes, past `ALERT_SCAN_MAX`, so routing stops before matching.
    /// The label mixes `a` and `b` without a period, so the lazy DFA cannot hold its states and
    /// gives way to the PikeVM. Measured on 2026-11-08 in a debug build: 0.05 s, where matching
    /// took 46 s.
    #[test]
    fn a_long_label_scanned_by_a_large_expression_stops_at_the_evaluation_ceiling() {
        let mut seed = 1u64;
        let value = (0..300_000)
            .map(|_| {
                seed = (seed.wrapping_mul(6_364_136_223_846_793_005)).wrapping_add(1);
                if seed >> 63 == 0 { 'a' } else { 'b' }
            })
            .collect::<String>();
        let source = serde_json::json!({
            "groups":[{"name":"long", "rules":[{"alert":"Long", "expr":"up", "labels":{"support":value}}]}],
            "route":{"receiver":"default", "routes":[{"receiver":"team", "match_re":{"support":".*a.{1000}a.*"}}]},
            "receivers":[{"name":"default"}, {"name":"team"}]
        })
        .to_string();
        assert!(source.len() < crate::lang::MAX_BYTES);
        let repo = Repository {
            identity: "example.com/scan".into(),
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
        let took = started.elapsed();
        eprintln!("long label scan evaluation limit: {took:?}");
        assert!(links.is_empty(), "{} links", links.len());
        let status = Alert.scope_status(&repo, &exports[0], &exports, &links);
        assert_eq!(status, ["alert routing skipped: evaluation limit"]);
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
    }

    /// A condition is charged, before it reads a value, the steps matching may take, and no unit
    /// of `ALERT_MATCH_WORK_MAX`: `.*a{999}` may run its 1,001 positions (the loop of `.*`
    /// included) at each of the 128 offsets of a label of 127 bytes, 128,128 steps, which a budget
    /// of 128,128 pays and one of 128,127 does not. A substring search costs the bytes it reads and
    /// the tags of a condition are charged together. On a short value, `[a-z]+-(?:prod|staging)`
    /// runs its 15 positions at each of 12 offsets, and a literal alternation costs its characters
    /// and its choice.
    #[test]
    fn a_condition_is_charged_what_matching_may_cost_before_it_reads_a_value() {
        let matcher = |op, key: &str, pattern: &str| Matcher {
            key: key.into(),
            value: pattern.into(),
            op,
            tags: key == "tags",
            field: String::new(),
            tag_value: pattern.into(),
        };
        let rule = |value: &str, tags: Vec<String>| -> Rule {
            let stream = if tags.is_empty() {
                "Labels"
            } else {
                "Opsgenie"
            };
            serde_json::from_value(
                serde_json::json!({"key":"r", "name":"R", "line":1, "end_line":1,
                "labels":{"support":value}, "unknown":[], "tags":tags, "stream":stream}),
            )
            .unwrap()
        };
        let run = |matcher: &Matcher, rule: &Rule, steps: usize| {
            let regex = regexp::Pass::new(1).compile(matcher.pattern(), 1);
            let mut work = Work { units: 0, steps };
            let matched = Condition { matcher, regex }.matches(rule, &mut work);
            matched.map(|matched| (matched, work.units, work.steps))
        };
        let long = rule(&"b".repeat(127), Vec::new());
        let expression = matcher(Op::Regex, "support", ".*a{999}");
        assert!(matches!(
            run(&expression, &long, 128_128),
            Ok((Some(false), 0, 0))
        ));
        assert!(matches!(run(&expression, &long, 128_127), Err(Limit::Work)));
        let search = matcher(Op::Contains, "support", "a");
        assert!(matches!(run(&search, &long, 127), Ok((Some(false), 0, 0))));
        assert!(matches!(run(&search, &long, 126), Err(Limit::Work)));
        let tagged = rule(
            "",
            (0..4).map(|i| format!("{i}{}", "b".repeat(30))).collect(),
        );
        let tags = matcher(Op::Regex, "tags", ".*");
        assert!(matches!(run(&tags, &tagged, 256), Ok((Some(true), 0, 0))));
        assert!(matches!(run(&tags, &tagged, 255), Err(Limit::Work)));
        let short = rule("orders-prod", Vec::new());
        let ordinary = matcher(Op::Regex, "support", "[a-z]+-(?:prod|staging)");
        assert!(matches!(
            run(&ordinary, &short, 180),
            Ok((Some(true), 0, 0))
        ));
        assert!(matches!(run(&ordinary, &short, 179), Err(Limit::Work)));
        let literal = matcher(Op::Regex, "support", "orders-prod|orders-staging");
        assert!(matches!(run(&literal, &short, 26), Ok((Some(true), 0, 0))));
        assert!(matches!(run(&literal, &short, 25), Err(Limit::Work)));
    }

    #[test]
    fn regex_source_and_automaton_ceilings_leave_large_matchers_unmatched() {
        assert!(regex(&"a".repeat(ALERT_REGEX_MAX)).is_some());
        assert!(regex(&"a".repeat(ALERT_REGEX_MAX + 1)).is_none());
        assert!(regex("a{100000000}").is_none());
        assert!(regex("[").is_none());
    }

    /// An expression Meada leaves unmatched for its own budgets compiles in Go, so it is no
    /// refusal of the configuration that holds it (#718): the source length, the automaton and
    /// case folding, here as far as one expression of `ALERT_REGEX_MAX` bytes goes.
    #[test]
    fn an_expression_past_a_budget_is_valid_for_go_and_no_refusal() {
        for pattern in [
            "a".repeat(ALERT_REGEX_MAX + 1),
            "x{1000}".repeat(80),
            format!("(?i){}", r"\p{Any}".repeat(4)),
        ] {
            assert!(regex(&pattern).is_none(), "{}", &pattern[..20]);
            assert_eq!(regexp::refusal(&pattern), None, "{}", &pattern[..20]);
        }
    }

    /// Folding the case of `\p{Any}` visits each of its 1,112,064 code points: three fit
    /// `ALERT_REGEX_FOLD_MAX`, a fourth does not, nor do two in each of two expressions sharing
    /// it. `\P{Any}` folds `\p{Any}` before negating it; a bracket folds all it holds, here
    /// everything but the digits (about as many), and each Unicode class in it on its own. The
    /// lowercase letters folding `\p{Lu}` adds join the range written after the last of them,
    /// almost a million uncased code points that the bracket then visits.
    #[test]
    fn case_folding_past_the_fold_ceiling_leaves_a_matcher_unmatched() {
        let any = 1_112_064;
        assert!(3 * any <= ALERT_REGEX_FOLD_MAX && 4 * any > ALERT_REGEX_FOLD_MAX);
        let compiles = |class: &str, n: usize, shares: usize| {
            let pattern = format!("(?i){}", class.repeat(n));
            regexp::Pass::new(1).compile(&pattern, shares).is_some()
        };
        assert_eq!(
            [
                compiles(r"\p{Any}", 3, 1),
                compiles(r"\p{Any}", 4, 1),
                compiles(r"\p{Any}", 2, 2),
                compiles(r"\P{Any}", 4, 1),
                compiles(r"[a\D]", 3, 1),
                compiles(r"[a\D]", 4, 1),
                compiles(r"[\p{Any}\p{Any}\p{Any}\p{Any}]", 1, 1),
                regex(&r"\p{Any}".repeat(4)).is_some(),
                compiles(r"[\x{1E944}-\x{10FFFF}\p{Lu}]", 3, 1),
                compiles(r"[\x{1E944}-\x{10FFFF}\p{Lu}]", 5, 1),
            ],
            [
                true, false, false, false, true, false, false, true, true, false
            ]
        );
        assert!(regex(r"(?i)\p{Lu}").unwrap().is_match("a"));
    }

    /// Folding visits only the ranges that hold a cased code point, of classes not folded yet:
    /// the Perl classes `regex_source` writes as brackets of uncased characters, a bracket of
    /// only such classes and `\p{Han}` visit nothing, where they were counted whole.
    #[test]
    fn case_folding_counts_only_ranges_that_fold() {
        let compiles = |class: &str, n: usize| regex(&format!("(?i){}", class.repeat(n))).is_some();
        assert_eq!(
            [
                compiles(r"[\S]", 4),
                compiles(r"[\s\S]", 4),
                compiles(r"\p{Han}", 43),
            ],
            [true, true, true]
        );
        assert!(regex(r"(?i)[\S]+").unwrap().is_match("ÀB"));
    }

    /// The export of the repository `identity`: a rule labelled `team: payments`, and a route
    /// configuration `am{i}.yaml` routing `team` by each of `patterns`.
    fn routed(identity: &str, patterns: &[String]) -> (Repository, Export) {
        let rule = serde_json::json!({"groups":[{"name":"g", "rules":[{"alert":"A", "expr":"up", "labels":{"team":"payments"}}]}]});
        let mut configurations = vec![(
            "rules.yaml".to_string(),
            serde_json::json!({"alert":config::read("rules.yaml", rule.to_string().as_bytes())}),
        )];
        for (i, pattern) in patterns.iter().enumerate() {
            let path = format!("am{i}.yaml");
            let source = serde_json::json!({"route":{"receiver":"default", "routes":[{"receiver":"team", "match_re":{"team":pattern}}]}, "receivers":[{"name":"default"}, {"name":"team"}]});
            let summary = config::read(&path, source.to_string().as_bytes());
            configurations.push((path, serde_json::json!({ "alert": summary })));
        }
        let repo = Repository {
            identity: identity.into(),
            root: Default::default(),
            config: Default::default(),
            decisions: Default::default(),
            index: Default::default(),
        };
        let mut export = Export {
            repository: repo.identity.clone(),
            configurations,
            ..Default::default()
        };
        Alert.declare(&repo, &mut export);
        (repo, export)
    }

    /// The route configurations a rule of `workspace` links to, as `repository::path`.
    fn matched(workspace: Vec<(Repository, Export)>) -> BTreeSet<String> {
        let (repos, exports): (Vec<_>, Vec<_>) = workspace.into_iter().unzip();
        let providers = (repos.iter().zip(&exports))
            .flat_map(|(repo, export)| export.providers.iter().map(move |p| (repo, p)))
            .collect::<Vec<_>>();
        let links = Alert.workspace_links(&exports, &providers).unwrap();
        (links.iter())
            .filter_map(|link| {
                let mut parts = link.consumer.split("::");
                Some(format!("{}::{}", parts.next()?, parts.next()?))
            })
            .collect()
    }

    /// Each routing scope of a linking pass compiles its expressions within an equal part of the
    /// ceilings, so what links does not depend on the repository asking, whose scopes come first:
    /// of two route configurations, each fits one case-insensitive `\p{Any}` (1,112,064 folds) in
    /// its half of `ALERT_REGEX_FOLD_MAX`, and neither fits two, in either order.
    #[test]
    fn routing_scopes_compile_within_equal_parts_in_any_order() {
        let any = 1_112_064;
        assert!(any <= ALERT_REGEX_FOLD_MAX / 2 && 2 * any > ALERT_REGEX_FOLD_MAX / 2);
        let linked = |pattern: &str, order: [&str; 2]| {
            let workspace =
                order.map(|identity| routed(identity, &[format!("{pattern}|{identity}")]));
            matched(workspace.into())
        };
        let [a, b] = ["example.com/a", "example.com/b"];
        let both = BTreeSet::from([format!("{a}::am0.yaml"), format!("{b}::am0.yaml")]);
        for order in [[a, b], [b, a]] {
            assert_eq!(linked(r"(?i)\p{Any}+", order), both);
            assert_eq!(linked(r"(?i)(?:\p{Any}|\p{Any})+", order), BTreeSet::new());
        }
    }

    /// A pattern of hundreds of case-insensitive `\p{Any}`, whose folding took minutes, is
    /// refused by counting what folding would visit, in linear time.
    #[test]
    fn hundreds_of_case_insensitive_classes_are_refused_in_linear_time() {
        let pattern = format!("(?i){}", r"\p{Any}".repeat((ALERT_REGEX_MAX - 4) / 7));
        let started = std::time::Instant::now();
        assert!(regex(&pattern).is_none());
        let took = started.elapsed();
        eprintln!("hundreds of case-insensitive classes: {took:?}");
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    }

    /// The automata of a linking pass share `ALERT_REGEX_POOL_MAX` in equal parts per routing
    /// scope, so many route configurations no longer compile `ALERT_REGEX_SIZE_MAX` each:
    /// `\pL{1,24}` (1,030,136 bytes) links from one route configuration, and from none of 400,
    /// refused in linear time. Measured on 2026-11-08 in a debug build: 0.82 s, where compiling
    /// each in its own megabyte took 14.6 s.
    #[test]
    fn many_route_configurations_share_the_automaton_pool() {
        let patterns = (0..400)
            .map(|i| format!(r"\pL{{1,24}}|{i}"))
            .collect::<Vec<_>>();
        let one = matched(vec![routed("example.com/one", &patterns[..1])]);
        assert_eq!(one, BTreeSet::from(["example.com/one::am0.yaml".into()]));
        let started = std::time::Instant::now();
        let many = matched(vec![routed("example.com/many", &patterns)]);
        let took = started.elapsed();
        eprintln!("many route configurations: {took:?}");
        assert_eq!(many.len(), 0);
        assert!(took < std::time::Duration::from_secs(20), "{took:?}");
    }

    /// An expression holds its part of `ALERT_REGEX_CLASSES_MAX` Unicode classes, case folded or
    /// not: a quarter of them fits one of four expressions of a routing scope, or the expression
    /// of one of four scopes, and one more does not.
    #[test]
    fn unicode_classes_past_an_expressions_part_leave_it_unmatched() {
        let quarter = ALERT_REGEX_CLASSES_MAX / 4;
        let compiles = |classes: usize, scopes: usize, shares: usize| {
            let pattern = format!("[{}]", r"\pL".repeat(classes));
            regexp::Pass::new(scopes)
                .compile(&pattern, shares)
                .is_some()
        };
        assert_eq!(
            [
                compiles(quarter, 1, 4),
                compiles(quarter + 1, 1, 4),
                compiles(quarter, 4, 1),
                compiles(quarter + 1, 4, 1),
            ],
            [true, false, true, false]
        );
    }

    /// 240 expressions of 1,358 `\pL` each, a 1 MB route configuration whose classes took about a
    /// minute to read, are refused past their parts in linear time. Measured on 2026-11-08 in a
    /// debug build: 0.72 s, where compiling them all took 19.4 s.
    #[test]
    fn hundreds_of_expressions_of_unicode_classes_are_refused_in_linear_time() {
        let patterns = (0..240)
            .map(|i| format!("[{}]|z{i}", r"\pL".repeat(1358)))
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let mut pass = regexp::Pass::new(1);
        let compiled = (patterns.iter())
            .filter(|pattern| pass.compile(pattern, patterns.len()).is_some())
            .count();
        let took = started.elapsed();
        eprintln!("hundreds of expressions of Unicode classes: {took:?}");
        assert_eq!(compiled, 0);
        assert!(took < std::time::Duration::from_secs(20), "{took:?}");
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
        // Go refuses a repetition operator stacked on another; a group or a lazy `?` between is
        // no stacking.
        for pattern in ["a**", "a*+", "a+*", "a{2}{3}", "a{2}*", "a*??"] {
            assert!(regex(pattern).is_none(), "{pattern}");
        }
        for pattern in ["(?:a*)*", "(a+)?", "a*?", "a{2}?"] {
            assert!(regex(pattern).is_some(), "{pattern}");
        }
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
