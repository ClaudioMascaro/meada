//! Monitoring configuration facts, stored once and read without checkout access by queries.

use super::regexp;
use crate::document::events::{self, Step, Syntax};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

// ponytail: repeated labels and long route paths must not amplify one file without bound.
const ALERT_STORED_MAX: usize = 4 * crate::lang::MAX_BYTES;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Summary {
    pub rules: Vec<Rule>,
    pub routes: Vec<Route>,
    pub truncated: bool,
    #[serde(default)]
    pub forwardings: Vec<super::forwarding::Receiver>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Rule {
    pub key: String,
    pub name: String,
    pub line: usize,
    pub end_line: usize,
    pub labels: BTreeMap<String, String>,
    pub unknown: BTreeSet<String>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub unknown_tags: bool,
    #[serde(default)]
    pub stream: Stream,
    #[serde(default)]
    pub scopes: BTreeSet<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Stream {
    #[default]
    Labels,
    Opsgenie,
    Pagerduty,
}

impl Rule {
    pub fn emissions(&self) -> impl Iterator<Item = (&str, &str)> {
        self.labels
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .chain(self.tags.iter().filter_map(|value| tag(value)))
    }
}

pub(super) fn tag(value: &str) -> Option<(&str, &str)> {
    let (key, value) = value.split_once(':')?;
    (!key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !value.is_empty())
    .then_some((key, value))
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum Op {
    Equal,
    NotEqual,
    Regex,
    NotRegex,
    Prefix,
    Contains,
    Suffix,
    NotPrefix,
    NotContains,
    NotSuffix,
}
impl Op {
    pub fn text(self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::NotEqual => "!=",
            Self::Regex => "=~",
            Self::NotRegex => "!~",
            Self::Prefix => " prefix ",
            Self::Contains => " contains ",
            Self::Suffix => " suffix ",
            Self::NotPrefix => " not-prefix ",
            Self::NotContains => " not-contains ",
            Self::NotSuffix => " not-suffix ",
        }
    }
    pub fn regex(self) -> bool {
        matches!(self, Self::Regex | Self::NotRegex)
    }
    pub fn inferred(self) -> bool {
        !matches!(self, Self::Equal | Self::NotEqual)
    }
    pub fn negative(self) -> bool {
        matches!(
            self,
            Self::NotEqual | Self::NotRegex | Self::NotPrefix | Self::NotContains | Self::NotSuffix
        )
    }
    pub fn negated(self) -> Self {
        match self {
            Self::Equal => Self::NotEqual,
            Self::NotEqual => Self::Equal,
            Self::Regex => Self::NotRegex,
            Self::NotRegex => Self::Regex,
            Self::Prefix => Self::NotPrefix,
            Self::NotPrefix => Self::Prefix,
            Self::Contains => Self::NotContains,
            Self::NotContains => Self::Contains,
            Self::Suffix => Self::NotSuffix,
            Self::NotSuffix => Self::Suffix,
        }
    }
    fn read(text: &str) -> Option<Self> {
        Some(match text {
            "=" => Self::Equal,
            "!=" => Self::NotEqual,
            "=~" => Self::Regex,
            "!~" => Self::NotRegex,
            _ => return None,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Matcher {
    pub key: String,
    pub value: String,
    pub op: Op,
    #[serde(default)]
    pub tags: bool,
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub tag_value: String,
}
impl Matcher {
    pub fn expression(&self) -> String {
        let key = if self.tags {
            "tags"
        } else if self.field.is_empty() {
            &self.key
        } else {
            &self.field
        };
        format!(
            "{}{}{}",
            serde_json::to_string(key).unwrap_or_default(),
            self.op.text(),
            serde_json::to_string(self.pattern()).unwrap_or_default()
        )
    }
    pub fn via(&self) -> String {
        format!("alert matcher {}", self.op.text())
    }
    pub fn contract(&self) -> String {
        super::contract(&self.key, &self.value)
    }
    pub fn pattern(&self) -> &str {
        if self.tags {
            &self.tag_value
        } else {
            &self.value
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Route {
    pub key: String,
    pub receiver: String,
    pub line: usize,
    pub end_line: usize,
    pub matchers: Vec<Matcher>,
    pub parent: Option<usize>,
    pub continuing: bool,
    pub valid: bool,
    #[serde(default)]
    pub stream: Stream,
    #[serde(default)]
    pub any: bool,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub document: usize,
    #[serde(default)]
    pub order: Option<usize>,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub set: String,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub logic: Vec<super::pcl::Test>,
    #[serde(default)]
    pub opaque: bool,
    #[serde(default)]
    pub barrier: bool,
    #[serde(default)]
    pub unknown_scope: bool,
    /// An AlertmanagerConfig route: prometheus-operator also matches its own namespace, which
    /// Meada does not read, so its links are `inferred`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub namespaced: bool,
}
const NAMESPACED: &str = " (AlertmanagerConfig namespace)";
impl Route {
    pub fn via(&self, matcher: &Matcher) -> String {
        if self.namespaced {
            format!("{}{NAMESPACED}", matcher.via())
        } else {
            matcher.via()
        }
    }
}

pub(super) type Spans = BTreeMap<(usize, String), (usize, usize)>;

pub(super) fn is_file(path: &str) -> bool {
    !crate::config_values::helm_values(path)
        && !path.split('/').any(|part| part == "templates")
        && !crate::config_values::lockfile(path)
        && [".yaml", ".yml", ".json", ".tf", ".tofu"]
            .iter()
            .any(|s| path.ends_with(s))
}
pub(super) fn literal(value: &str) -> bool {
    !value.contains("{{") && !value.contains("${") && !value.contains("%{")
}
fn label_set(value: Option<&Value>) -> (BTreeMap<String, String>, BTreeSet<String>) {
    let mut known = BTreeMap::new();
    let mut unknown = BTreeSet::new();
    let computed_key = value
        .and_then(Value::as_object)
        .is_some_and(|object| object.keys().any(|key| !literal(key)));
    for (key, value) in value.and_then(Value::as_object).into_iter().flatten() {
        if let Some(value) = value.as_str().filter(|v| literal(v)) {
            known.insert(key.clone(), value.into());
        } else {
            unknown.insert(key.clone());
        }
    }
    if computed_key {
        unknown.extend(known.keys().cloned());
        known.clear();
    }
    (known, unknown)
}

pub(super) fn read(path: &str, src: &[u8]) -> Summary {
    let mut out = Summary::default();
    if src.len() > crate::lang::MAX_BYTES || !is_file(path) {
        return out;
    }
    if path.ends_with(".tf") || path.ends_with(".tofu") {
        return terraform(path, src);
    }
    let mut spans = Spans::new();
    let Ok(docs) = events::read(Syntax::of(path), src, |s| {
        if s.path.first() == Some(&Step::Key("resource".into())) {
            let mut keys = 0;
            for (i, step) in s.path.iter().enumerate() {
                if let Step::Key(key) = step {
                    keys += 1;
                    if keys == 3 || (keys > 3 && matches!(key.as_str(), "set" | "rule")) {
                        let end =
                            i + 1 + usize::from(matches!(s.path.get(i + 1), Some(Step::Index(_))));
                        let key = crate::config_values::key_path(&s.path[..end]);
                        let span = spans.entry((s.document, key)).or_insert((s.line, s.line));
                        span.0 = span.0.min(s.line);
                        span.1 = span.1.max(s.line);
                    }
                }
            }
            return;
        }
        let end = s
            .path
            .windows(2)
            .enumerate()
            .filter_map(|(i, pair)| match pair {
                [Step::Key(key), Step::Index(_)]
                    if matches!(key.as_str(), "rules" | "routes" | "rule" | "set") =>
                {
                    Some(i + 2)
                }
                _ => None,
            })
            .next_back()
            .or_else(|| {
                s.path
                    .iter()
                    .position(|p| *p == Step::Key("route".into()))
                    .map(|i| i + 1)
            });
        if let Some(end) = end {
            let key = crate::config_values::key_path(&s.path[..end]);
            let span = spans.entry((s.document, key)).or_insert((s.line, s.line));
            span.0 = span.0.min(s.line);
            span.1 = span.1.max(s.line);
        }
    }) else {
        return out;
    };
    let mut bytes = 0;
    for (d, doc) in docs.iter().enumerate() {
        let prefix = if docs.len() > 1 {
            format!("[{d}].")
        } else {
            String::new()
        };
        // Alertmanager loads none of a configuration whose matcher holds a regular expression Go
        // refuses, so it gives no rule, route or receiver of its own.
        if refused(doc).is_some() {
            continue;
        }
        let object = doc.get("kind").and_then(Value::as_str);
        let monitoring = monitoring(doc);
        let (body, base) = if monitoring && object == Some("PrometheusRule") {
            (&doc["spec"], "spec.")
        } else {
            (doc, "")
        };
        if object.is_none() || (monitoring && object == Some("PrometheusRule")) {
            for (g, group) in body
                .get("groups")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                for (r, rule) in group
                    .get("rules")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let alert = rule
                        .get("alert")
                        .and_then(Value::as_str)
                        .filter(|_| rule.get("expr").is_some());
                    let name = alert.or_else(|| grafana_name(rule));
                    let Some(name) = name.filter(|n| !n.is_empty() && literal(n)) else {
                        continue;
                    };
                    if rule.get("labels").is_some_and(|v| !v.is_object()) {
                        continue;
                    }
                    let key = format!("{base}groups[{g}].rules[{r}]");
                    let (line, end_line) = spans.get(&(d, key.clone())).copied().unwrap_or((1, 1));
                    let (mut labels, mut unknown) = label_set(group.get("labels"));
                    let (own, unresolved) = label_set(rule.get("labels"));
                    for key in &unresolved {
                        labels.remove(key);
                    }
                    for key in own.keys() {
                        unknown.remove(key);
                    }
                    labels.extend(own);
                    unknown.extend(unresolved);
                    if alert.is_some() {
                        // Prometheus/Loki apply rule labels before setting the rule name.
                        labels.insert("alertname".into(), name.into());
                        unknown.remove("alertname");
                    }
                    let item = Rule {
                        key: format!("{prefix}{key}"),
                        name: name.into(),
                        line,
                        end_line,
                        labels,
                        unknown,
                        tags: BTreeSet::new(),
                        unknown_tags: false,
                        stream: Stream::Labels,
                        scopes: BTreeSet::new(),
                    };
                    if charge_rule(&item, path, &mut bytes) {
                        out.rules.push(item);
                    } else {
                        out.truncated = true;
                        return bounded(out);
                    }
                }
            }
            for (key, value) in std::iter::once(("rule".to_string(), doc)).chain(
                doc.as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(i, rule)| (format!("[{i}]"), rule)),
            ) {
                if let Some(name) = grafana_name(value) {
                    let (labels, unknown) = label_set(value.get("labels"));
                    let item = Rule {
                        key: format!("{prefix}{key}"),
                        name: name.into(),
                        line: 1,
                        end_line: src.iter().filter(|b| **b == b'\n').count().max(1),
                        labels,
                        unknown,
                        tags: BTreeSet::new(),
                        unknown_tags: false,
                        stream: Stream::Labels,
                        scopes: BTreeSet::new(),
                    };
                    if charge_rule(&item, path, &mut bytes) {
                        out.rules.push(item);
                    } else {
                        out.truncated = true;
                        return bounded(out);
                    }
                }
            }
        }
        if path.ends_with(".tf.json") || path.ends_with(".tofu.json") {
            terraform_json(path, doc, d, &spans, &mut out, &mut bytes);
        }
        let routed = alertmanager(doc);
        let (body, base) = routed.unwrap_or((doc, ""));
        if routed.is_some()
            && body.get("receivers").is_some_and(Value::is_array)
            && let Some(route) = body.get("route")
        {
            // Queue only input references and indexes: copying a parent's long receiver or
            // path across all children would allocate before the child's budget check.
            let mut pending = vec![(route, None, 0)];
            while let Some((value, parent, child_index)) = pending.pop() {
                let (key, inherited) = match parent {
                    Some(index) => {
                        let ancestor: &Route = &out.routes[index];
                        (
                            format!("{}.routes[{child_index}]", ancestor.key),
                            ancestor.receiver.as_str(),
                        )
                    }
                    None => (format!("{prefix}{base}route"), ""),
                };
                let receiver = value
                    .get("receiver")
                    .and_then(Value::as_str)
                    .unwrap_or(inherited)
                    .to_string();
                let conditions = matchers(value);
                let valid = value.is_object()
                    && value.get("routes").is_none_or(Value::is_array)
                    && value.get("receiver").is_none_or(Value::is_string)
                    && conditions.is_some()
                    && !receiver.is_empty()
                    && literal(&receiver)
                    && value.get("continue").is_none_or(Value::is_boolean);
                let span_key = key.strip_prefix(&prefix).unwrap_or(&key);
                let (line, end_line) = spans
                    .get(&(d, span_key.to_string()))
                    .copied()
                    .unwrap_or((1, 1));
                let item = Route {
                    key,
                    receiver,
                    line,
                    end_line,
                    matchers: conditions.unwrap_or_default(),
                    parent,
                    continuing: value
                        .get("continue")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    valid,
                    stream: Stream::Labels,
                    any: false,
                    group_by: value
                        .get("group_by")
                        .or_else(|| value.get("groupBy"))
                        .and_then(Value::as_array)
                        .and_then(|v| {
                            v.iter()
                                .map(|v| v.as_str().map(String::from))
                                .collect::<Option<Vec<_>>>()
                        })
                        .unwrap_or_else(|| {
                            if value
                                .get("group_by")
                                .or_else(|| value.get("groupBy"))
                                .is_some()
                            {
                                Vec::new()
                            } else {
                                parent.map_or_else(Vec::new, |p| out.routes[p].group_by.clone())
                            }
                        }),
                    document: d,
                    order: None,
                    group: String::new(),
                    set: String::new(),
                    next: None,
                    logic: Vec::new(),
                    opaque: false,
                    barrier: false,
                    unknown_scope: false,
                    namespaced: !base.is_empty(),
                };
                if !charge(&item, &mut bytes) || !charge_route(&item, path, &out.routes, &mut bytes)
                {
                    out.truncated = true;
                    return bounded(out);
                }
                let parent = Some(out.routes.len());
                out.routes.push(item);
                for (i, child) in array(value.get("routes")).iter().enumerate().rev() {
                    pending.push((child, parent, i));
                }
            }
        }
        super::forwarding::read(body, d, &mut out, &mut bytes);
    }
    bounded(out)
}
/// Whether `doc` is a prometheus-operator object.
fn monitoring(doc: &Value) -> bool {
    doc.get("apiVersion")
        .and_then(Value::as_str)
        .is_some_and(|v| v.starts_with("monitoring.coreos.com/"))
}

/// `doc` as an Alertmanager configuration: its own body, or the spec of a prometheus-operator
/// `AlertmanagerConfig` with the prefix of its key paths; `None` for any other object.
fn alertmanager(doc: &Value) -> Option<(&Value, &'static str)> {
    match doc.get("kind").and_then(Value::as_str) {
        None => Some((doc, "")),
        Some("AlertmanagerConfig") if monitoring(doc) => Some((&doc["spec"], "spec.")),
        Some(_) => None,
    }
}

/// Why Alertmanager refuses to load the configuration `doc`, if one of its route matchers holds a
/// regular expression Go's `regexp` certainly refuses (`regexp::refusal`). An expression that only
/// Meada cannot read, or reads past its budgets, is not one.
pub(super) fn refused(doc: &Value) -> Option<String> {
    let (body, _) = alertmanager(doc)?;
    if !body.get("receivers").is_some_and(Value::is_array) {
        return None;
    }
    let route = body.get("route")?;
    // Alertmanager compiles the matchers of its inhibit rules as it does those of its routes.
    let inhibits = ["inhibit_rules", "inhibitRules"]
        .into_iter()
        .flat_map(|field| array(body.get(field)))
        .flat_map(|rule| {
            ["source", "target"].into_iter().flat_map(move |side| {
                let name = |suffix| format!("{side}{suffix}");
                [
                    matchers_in(
                        rule,
                        [&name("_match"), &name("_match_re")],
                        &name("_matchers"),
                    ),
                    matchers_in(rule, ["", ""], &name("Match")),
                ]
            })
        });
    let mut pending = vec![route];
    let mut seen = BTreeSet::new();
    let mut check = |found: Option<Vec<Matcher>>| {
        found.into_iter().flatten().find_map(|matcher| {
            if !matcher.op.regex() || !seen.insert(matcher.value.clone()) {
                return None;
            }
            let code = regexp::refusal(&matcher.value)?;
            Some(format!(
                "{}{}{:?}: Go's regexp refuses it ({code})",
                matcher.key,
                matcher.op.text(),
                matcher.value
            ))
        })
    };
    for found in inhibits {
        if let Some(why) = check(found) {
            return Some(why);
        }
    }
    while let Some(route) = pending.pop() {
        if let Some(why) = check(matchers(route)) {
            return Some(why);
        }
        pending.extend(array(route.get("routes")));
    }
    None
}
pub(super) fn array(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn grafana_name(rule: &Value) -> Option<&str> {
    rule.get("uid")?.as_str()?;
    rule.get("condition")?.as_str()?;
    rule.get("data")?.as_array()?;
    rule.get("title")?.as_str()
}
fn matchers(route: &Value) -> Option<Vec<Matcher>> {
    matchers_in(route, ["match", "match_re"], "matchers")
}
/// The matchers `route` writes in the maps `equal` and `regex` (names to values) and in the list
/// `list` (strings, or objects with a `name`, a `value` and a `matchType` or `regex`).
fn matchers_in(route: &Value, [equal, regex]: [&str; 2], list: &str) -> Option<Vec<Matcher>> {
    let mut out = Vec::new();
    for (field, op) in [(equal, Op::Equal), (regex, Op::Regex)] {
        if let Some(value) = route.get(field) {
            for (key, value) in value.as_object()? {
                out.push(Matcher {
                    key: key.clone(),
                    value: value.as_str()?.into(),
                    op,
                    tags: false,
                    field: String::new(),
                    tag_value: String::new(),
                });
            }
        }
    }
    if let Some(value) = route.get(list) {
        for value in value.as_array()? {
            if let Some(text) = value.as_str() {
                out.extend(expressions(text)?);
            } else {
                let op = if let Some(ty) = value.get("matchType") {
                    Op::read(ty.as_str()?)?
                } else if match value.get("regex") {
                    Some(regex) => regex.as_bool()?,
                    None => false,
                } {
                    Op::Regex
                } else {
                    Op::Equal
                };
                out.push(Matcher {
                    key: value.get("name")?.as_str()?.into(),
                    value: value.get("value")?.as_str()?.into(),
                    op,
                    tags: false,
                    field: String::new(),
                    tag_value: String::new(),
                });
            }
        }
    }
    out.iter()
        .all(|m| !m.key.is_empty() && literal(&m.value))
        .then_some(out)
}

// A quoted key or value uses Alertmanager's escapes (see `token`). Commas inside strings are
// literal.
fn expressions(text: &str) -> Option<Vec<Matcher>> {
    let text = text.trim();
    let text = if text.starts_with('{') {
        text.strip_prefix('{')?.strip_suffix('}')?
    } else {
        text
    };
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    let mut out = Vec::new();
    for (i, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ',')))
    {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
        }
        if ch == ',' && !quoted {
            out.push(expression(text[start..i].trim())?);
            start = i + 1;
        }
    }
    (!quoted && !escaped).then_some(out)
}
fn expression(text: &str) -> Option<Matcher> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
        }
        if !quoted && matches!(ch, '=' | '!') {
            let suffix = &text[i..];
            let op = ["!=", "=~", "!~", "="]
                .into_iter()
                .find(|op| suffix.starts_with(op))?;
            let key = token(text[..i].trim())?;
            let value = token(suffix[op.len()..].trim())?;
            return Some(Matcher {
                key,
                value,
                op: Op::read(op)?,
                tags: false,
                field: String::new(),
                tag_value: String::new(),
            });
        }
    }
    None
}
fn token(text: &str) -> Option<String> {
    if let Some(quoted) = text.strip_prefix('"') {
        // Alertmanager's escapes: `\n`, `\"` and `\\`; any other keeps its backslash.
        let mut out = String::with_capacity(quoted.len());
        let mut chars = quoted.strip_suffix('"')?.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return None,
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    c @ ('"' | '\\') => out.push(c),
                    c => out.extend(['\\', c]),
                },
                c => out.push(c),
            }
        }
        Some(out)
    } else {
        // Alertmanager's classic parser unescapes `\\` in an unquoted value too, which this does
        // not: such a value is not read, and so cannot refuse a file by what Go never sees.
        (!text.is_empty()
            && !text.contains(['"', '=', '!', '{', '}'])
            && !text.contains("\\\\")
            && !text.chars().any(char::is_whitespace))
        .then(|| text.into())
    }
}

pub(super) fn charge(item: &impl Serialize, bytes: &mut usize) -> bool {
    let cost = serde_json::to_vec(item).map_or(ALERT_STORED_MAX, |v| v.len() + 1);
    *bytes = bytes.saturating_add(cost);
    *bytes <= ALERT_STORED_MAX.saturating_sub(64)
}
/// Charge the side fields a summary will materialize as well as its serialized bytes.
/// Ancestors are stored once but can contribute a matcher to many descendant routes.
fn fields(bytes: &mut usize, parts: &[usize]) -> bool {
    for part in parts {
        *bytes = bytes.saturating_add(*part);
    }
    *bytes <= ALERT_STORED_MAX.saturating_sub(64)
}
pub(super) fn charge_rule(rule: &Rule, path: &str, bytes: &mut usize) -> bool {
    charge(rule, bytes)
        && charge_node(path, &rule.key, "AlertRule", &rule.name, bytes)
        && rule.emissions().all(|(key, value)| {
            fields(
                bytes,
                &[
                    path.len(),
                    rule.key.len(),
                    key.len(),
                    value.len(),
                    "::::alert::::".len(),
                ],
            )
        })
}
pub(super) fn charge_route(item: &Route, path: &str, routes: &[Route], bytes: &mut usize) -> bool {
    if !charge_node(path, &item.key, "AlertmanagerRoute", &item.receiver, bytes) {
        return false;
    }
    let mut current = item;
    loop {
        for matcher in &current.matchers {
            if !fields(
                bytes,
                &[
                    path.len(),
                    item.key.len(),
                    matcher.key.len(),
                    matcher.value.len(),
                    "::::alert::::alert matcher !~".len(),
                    if item.namespaced { NAMESPACED.len() } else { 0 },
                ],
            ) {
                return false;
            }
        }
        let Some(parent) = current.parent else {
            return true;
        };
        current = &routes[parent];
    }
}

fn charge_node(path: &str, key: &str, ty: &str, name: &str, bytes: &mut usize) -> bool {
    fields(
        bytes,
        &[
            path.len(),
            path.len(),
            key.len(),
            key.len(),
            ty.len(),
            name.len(),
        ],
    )
}

fn bounded(out: Summary) -> Summary {
    if out.truncated {
        Summary {
            truncated: true,
            ..Default::default()
        }
    } else {
        out
    }
}

fn terraform(path: &str, src: &[u8]) -> Summary {
    use hcl_edit::Span;
    use hcl_edit::expr::{Expression, ObjectKey};
    let mut out = Summary::default();
    let Ok(text) = std::str::from_utf8(src) else {
        return out;
    };
    if crate::config_values::nested_past(text).is_some()
        || crate::config_values::wide_past(text).is_some()
    {
        return out;
    }
    let Ok(body) = hcl_edit::parser::parse_body(text) else {
        return out;
    };
    let newlines = text
        .bytes()
        .enumerate()
        .filter(|(_, b)| *b == b'\n')
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let line = |offset| newlines.partition_point(|&n| n < offset) + 1;
    let string = |expression: &Expression| match expression {
        Expression::String(s) => Some(s.value().clone()),
        _ => None,
    };
    let mut bytes = 0;
    for group in body.blocks().filter(|b| {
        b.ident.as_str() == "resource"
            && b.labels.len() == 2
            && b.labels[0].as_str() == "grafana_rule_group"
            && ["count", "for_each"]
                .iter()
                .all(|m| b.body.get_attribute(m).is_none())
    }) {
        let rules = group
            .body
            .blocks()
            .filter(|b| b.ident.as_str() == "rule")
            .collect::<Vec<_>>();
        for (i, rule) in rules.iter().enumerate() {
            let Some(name) = rule
                .body
                .get_attribute("name")
                .and_then(|a| string(&a.value))
            else {
                continue;
            };
            let Some(Expression::Object(labels)) =
                rule.body.get_attribute("labels").map(|a| &a.value)
            else {
                continue;
            };
            let mut known = BTreeMap::new();
            let mut unknown = BTreeSet::new();
            let mut computed_key = false;
            for (key, value) in labels {
                let key = match key {
                    ObjectKey::Ident(id) => Some(id.as_str().to_string()),
                    ObjectKey::Expression(expr) => string(expr),
                };
                let Some(key) = key else {
                    computed_key = true;
                    continue;
                };
                if let Some(value) = string(value.expr()).filter(|v| literal(v)) {
                    known.insert(key, value);
                } else {
                    unknown.insert(key);
                }
            }
            if computed_key {
                unknown.extend(known.keys().cloned());
                known.clear();
            }
            let mut steps = vec![
                Step::Key("resource".into()),
                Step::Key("grafana_rule_group".into()),
                Step::Key(group.labels[1].as_str().into()),
                Step::Key("rule".into()),
            ];
            if rules.len() > 1 {
                steps.push(Step::Index(i));
            }
            let span = rule.span().unwrap_or(0..0);
            let item = Rule {
                key: crate::config_values::key_path(&steps),
                name,
                line: line(span.start),
                end_line: line(span.end.saturating_sub(1)),
                labels: known,
                unknown,
                tags: BTreeSet::new(),
                unknown_tags: false,
                stream: Stream::Labels,
                scopes: BTreeSet::new(),
            };
            if charge_rule(&item, path, &mut bytes) {
                out.rules.push(item);
            } else {
                out.truncated = true;
                return bounded(out);
            }
        }
    }
    super::incident::terraform(path, &body, text, &mut out, &mut bytes);
    bounded(out)
}
fn terraform_json(
    path: &str,
    doc: &Value,
    document: usize,
    spans: &Spans,
    out: &mut Summary,
    bytes: &mut usize,
) {
    for (name, group) in doc
        .pointer("/resource/grafana_rule_group")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, group)| group.get("count").is_none() && group.get("for_each").is_none())
    {
        for (i, rule) in array(group.get("rule")).iter().enumerate() {
            let Some(title) = rule.get("name").and_then(Value::as_str) else {
                continue;
            };
            let (labels, unknown) = label_set(rule.get("labels"));
            let key = crate::config_values::key_path(&[
                Step::Key("resource".into()),
                Step::Key("grafana_rule_group".into()),
                Step::Key(name.clone()),
                Step::Key("rule".into()),
                Step::Index(i),
            ]);
            let (line, end_line) = spans
                .get(&(document, key.clone()))
                .copied()
                .unwrap_or((1, 1));
            let item = Rule {
                key,
                name: title.into(),
                line,
                end_line,
                labels,
                unknown,
                tags: BTreeSet::new(),
                unknown_tags: false,
                stream: Stream::Labels,
                scopes: BTreeSet::new(),
            };
            if charge_rule(&item, path, bytes) {
                out.rules.push(item);
            } else {
                out.truncated = true;
            }
        }
    }
    super::incident::json(path, doc, document, spans, out, bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_receivers_stop_before_wide_route_queues_expand_them() {
        let source = serde_json::json!({"route":{"receiver":"x".repeat(400_000), "routes":vec![serde_json::json!({}); 50_000]}, "receivers":[]}).to_string();
        assert!(source.len() < crate::lang::MAX_BYTES);
        let started = std::time::Instant::now();
        let summary = read("alerts.json", source.as_bytes());
        assert!(summary.truncated);
        assert!(summary.rules.is_empty() && summary.routes.is_empty());
        eprintln!("wide receiver inheritance: {:?}", started.elapsed());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn projected_nodes_count_long_paths_even_without_matcher_sides() {
        let source = serde_json::json!({"route":{"receiver":"x", "routes":vec![serde_json::json!({}); 20000]}, "receivers":[]}).to_string();
        let path = format!("{}alerts.json", "part/".repeat(600));
        assert!(source.len() < crate::lang::MAX_BYTES);
        let started = std::time::Instant::now();
        let summary = read(&path, source.as_bytes());
        assert!(summary.truncated);
        assert!(summary.rules.is_empty() && summary.routes.is_empty());
        eprintln!("projected nodes with a long path: {:?}", started.elapsed());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn inherited_route_conditions_charge_the_sides_they_will_materialize() {
        let matchers = (0..2200)
            .map(|i| (format!("key{i}"), Value::String("team-b".into())))
            .collect::<serde_json::Map<_, _>>();
        let routes = (0..100)
            .map(|_| serde_json::json!({"receiver":"wide"}))
            .collect::<Vec<_>>();
        let source = serde_json::json!({"route":{"receiver":"wide", "match":matchers, "routes":routes}, "receivers":[{"name":"wide"}]}).to_string();
        assert!(source.len() < crate::lang::MAX_BYTES);
        let started = std::time::Instant::now();
        let summary = read("alerts.json", source.as_bytes());
        assert!(summary.truncated);
        assert!(summary.rules.is_empty() && summary.routes.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn repeated_grafana_groups_helm_values_and_matcher_escapes_read_as_their_tools_do() {
        // A rule group Terraform makes any number of times, zero included, declares no rule.
        let group = |meta: &str| {
            format!(
                "resource \"grafana_rule_group\" \"g\" {{\n{meta}  rule {{\n    name = \"A\"\n    \
                 labels = {{ support = \"team-b\" }}\n  }}\n}}\n"
            )
        };
        assert_eq!(read("alerts.tf", group("").as_bytes()).rules.len(), 1);
        for meta in ["  count = 0\n", "  for_each = {}\n"] {
            assert!(
                read("alerts.tf", group(meta).as_bytes()).rules.is_empty(),
                "{meta}"
            );
        }
        let json = |meta: &str| {
            format!(
                "{{\"resource\":{{\"grafana_rule_group\":{{\"g\":{{{meta}\"rule\":[{{\"name\":\"A\",\
                 \"labels\":{{\"support\":\"team-b\"}}}}]}}}}}}}}"
            )
        };
        assert_eq!(read("alerts.tf.json", json("").as_bytes()).rules.len(), 1);
        assert!(
            read("alerts.tf.json", json("\"count\":0,").as_bytes())
                .rules
                .is_empty()
        );
        // Helm values go by a `values` word anywhere in the file name.
        for helm in [
            "values.yaml",
            "values-prod.yaml",
            "prod.values.yaml",
            "ops/alert_values.yml",
        ] {
            assert!(!is_file(helm), "{helm}");
        }
        assert!(is_file("rules.yaml") && is_file("alert-valuesets.yaml"));
        // Alertmanager's escapes: `\n`, `\"` and `\\`; any other keeps its backslash.
        let read = expressions(r#"{a=~"db\.prod", b="say \"hi\"", c="x\\y", d="l\nm"}"#).unwrap();
        assert_eq!(
            read.iter().map(|m| m.value.as_str()).collect::<Vec<_>>(),
            [r"db\.prod", "say \"hi\"", r"x\y", "l\nm"]
        );
        assert!(expressions(r#"a="open"#).is_none());
    }

    /// Alertmanager loads none of a configuration with a regex Go refuses, but the objects
    /// beside it in a manifest bundle are other configurations: only its own document gives no
    /// route and no receiver (#718).
    #[test]
    fn a_document_go_refuses_gives_no_facts_but_its_neighbours_keep_theirs() {
        let rules = "apiVersion: monitoring.coreos.com/v1\nkind: PrometheusRule\nmetadata: {name: r}\n\
                     spec:\n  groups:\n    - name: g\n      rules:\n        - alert: Down\n          \
                     expr: up\n          labels: {support: team-b}\n";
        let manager = |regex: &str| {
            format!(
                "---\napiVersion: monitoring.coreos.com/v1alpha1\nkind: AlertmanagerConfig\n\
                 metadata: {{name: m}}\nspec:\n  receivers: [{{name: a}}]\n  route:\n    receiver: a\n    \
                 routes:\n      - receiver: a\n        matchers: [{{name: x, value: '{regex}', matchType: '=~'}}]\n"
            )
        };
        let counts = |regex: &str| {
            let summary = read(
                "alerts.yaml",
                format!("{rules}{}", manager(regex)).as_bytes(),
            );
            (summary.rules.len(), summary.routes.len())
        };
        assert_eq!(counts("a.*"), (1, 2));
        assert_eq!(counts("a["), (1, 0));
        // Past a budget of Meada's own, Go reads it.
        assert_eq!(
            counts(&"a".repeat(super::super::ALERT_REGEX_MAX + 1)),
            (1, 2)
        );
    }

    /// Alertmanager compiles the regexes of its inhibit rules too, and unescapes `\\` in an
    /// unquoted matcher value, which a regex judged as written would read differently.
    #[test]
    fn inhibit_rule_regexes_refuse_a_configuration_and_unquoted_escapes_are_not_judged() {
        let refused = |text: &str| {
            let docs = events::read(Syntax::Yaml, text.as_bytes(), |_| {}).unwrap();
            docs.iter().filter_map(refused).count()
        };
        let config = |route: &str, inhibit: &str| {
            format!("route: {{receiver: a{route}}}\nreceivers: [{{name: a}}]\n{inhibit}")
        };
        assert_eq!(refused(&config("", "")), 0);
        for inhibit in [
            "inhibit_rules: [{source_match_re: {x: 'a['}}]",
            "inhibit_rules: [{target_matchers: ['x=~\"(a\"']}]",
            "inhibit_rules: [{source_matchers: ['x=~*a']}]",
        ] {
            assert_eq!(refused(&config("", inhibit)), 1, "{inhibit}");
        }
        let operator = "apiVersion: monitoring.coreos.com/v1alpha1\nkind: AlertmanagerConfig\n\
                        spec:\n  receivers: [{name: a}]\n  route: {receiver: a}\n";
        let inhibit = "  inhibitRules: [{targetMatch: [{name: x, value: 'a(', matchType: '=~'}]}]";
        assert_eq!(refused(&format!("{operator}{inhibit}\n")), 1);
        assert_eq!(
            refused(&format!("{operator}{}\n", inhibit.replace("a(", "a"))),
            0
        );
        // A valid regex among inhibit rules, and equality values that are no regex.
        assert_eq!(
            refused(&config(
                "",
                "inhibit_rules: [{source_match: {x: 'a('}, target_match_re: {y: 'b+'}}]"
            )),
            0
        );
        // `\\(` is a literal backslash and a group for Go as written, and `\(` once unescaped.
        let unquoted = config(", routes: [{receiver: a, matchers: ['x=~a\\\\(b']}]", "");
        assert_eq!(refused(&unquoted), 0);
        assert!(
            read("alertmanager.yaml", unquoted.as_bytes()).routes[1..]
                .iter()
                .all(|r| !r.valid)
        );
        // Quoted, `\\` is unescaped as Alertmanager does: `a\\(b` reaches Go as `a\(b`, which is valid,
        // and `a\\\\(b` as `a\\(b`, which is not.
        let quoted = |value: &str| {
            config(
                &format!(", routes: [{{receiver: a, matchers: ['x=~\"{value}\"']}}]"),
                "",
            )
        };
        assert_eq!(refused(&quoted(r"a\\(b")), 0);
        assert_eq!(refused(&quoted(r"a\\\\(b")), 1);
    }

    #[test]
    fn a_file_of_thousands_of_small_rules_fits_the_storage_ceiling() {
        let rules = (0..2000)
            .map(|i| {
                format!("      - {{alert: Alert{i}, expr: up == 0, labels: {{severity: page, team: shop}}}}\n")
            })
            .collect::<String>();
        let text = format!("groups:\n  - name: many\n    rules:\n{rules}");
        assert!((150_000..200_000).contains(&text.len()), "{}", text.len());
        let summary = read("rules.yaml", text.as_bytes());
        assert!(!summary.truncated);
        assert_eq!(summary.rules.len(), 2000);
    }

    #[test]
    fn inherited_labels_past_the_storage_ceiling_leave_no_partial_alert_facts() {
        let rules = (0..1000)
            .map(|i| format!("      - {{alert: A{i}, expr: up}}\n"))
            .collect::<String>();
        let text = format!(
            "groups:\n  - name: large\n    labels: {{support: \"{}\"}}\n    rules:\n{rules}",
            "x".repeat(8192)
        );
        assert!(text.len() < crate::lang::MAX_BYTES);
        let started = std::time::Instant::now();
        let summary = read("rules.yaml", text.as_bytes());
        assert!(summary.truncated);
        assert!(summary.rules.is_empty() && summary.routes.is_empty());
        assert!(serde_json::to_vec(&summary).unwrap().len() < ALERT_STORED_MAX);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
