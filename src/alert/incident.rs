//! Literal incident-provider configuration. No provider or template code is executed.
use super::config::{self, Matcher, Op, Route, Rule, Spans, Stream, Summary};
use crate::document::events::Step;
use hcl_edit::Span;
use hcl_edit::expr::{Expression, ObjectKey};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

struct Source<'a> {
    path: &'a str,
    line: usize,
    end_line: usize,
    locations: &'a BTreeMap<String, (usize, usize)>,
}

pub(super) fn terraform(
    path: &str,
    body: &hcl_edit::structure::Body,
    text: &str,
    out: &mut Summary,
    bytes: &mut usize,
) {
    let newlines = text
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| (b == b'\n').then_some(i))
        .collect::<Vec<_>>();
    let line = |offset| newlines.partition_point(|&n| n < offset) + 1;
    for resource in body
        .blocks()
        .filter(|b| b.ident.as_str() == "resource" && b.labels.len() == 2)
    {
        let ty = resource.labels[0].as_str();
        if !known(ty) {
            continue;
        }
        let name = resource.labels[1].as_str();
        let span = resource.span().unwrap_or(0..0);
        let mut locations = BTreeMap::new();
        let value = literal_body(&resource.body, "", &line, &mut locations);
        read(
            Source {
                path,
                line: line(span.start),
                end_line: line(span.end.saturating_sub(1)),
                locations: &locations,
            },
            ty,
            name,
            &value,
            out,
            bytes,
        );
        if out.truncated {
            return;
        }
    }
}
fn known(ty: &str) -> bool {
    matches!(
        ty,
        "datadog_monitor"
            | "opsgenie_team_routing_rule"
            | "pagerduty_event_orchestration_service"
            | "pagerduty_event_orchestration_router"
            | "pagerduty_event_orchestration_global"
            | "pagerduty_event_orchestration_unrouted"
    )
}
fn literal_body(
    body: &hcl_edit::structure::Body,
    prefix: &str,
    line: &dyn Fn(usize) -> usize,
    locations: &mut BTreeMap<String, (usize, usize)>,
) -> Value {
    let mut object = Map::new();
    for attribute in body.attributes() {
        object.insert(
            attribute.key.as_str().into(),
            literal_value(&attribute.value),
        );
    }
    let mut counts = BTreeMap::new();
    for block in body.blocks() {
        // Dynamic blocks cannot establish a literal event shape.
        let key = block.ident.as_str();
        let index = counts.entry(key).or_insert(0usize);
        let at = format!("{prefix}.{key}[{index}]");
        *index += 1;
        if let Some(span) = block.span() {
            locations.insert(
                at.clone(),
                (line(span.start), line(span.end.saturating_sub(1))),
            );
        }
        let value = literal_body(&block.body, &at, line, locations);
        if let Some(array) = object
            .entry(key.to_string())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
        {
            array.push(value);
        }
    }
    Value::Object(object)
}
fn literal_value(value: &Expression) -> Value {
    match value {
        Expression::String(s) => Value::String(s.value().clone()),
        Expression::Bool(b) => Value::Bool(*b.value()),
        Expression::Number(n) => {
            serde_json::from_str(&n.value().to_string()).unwrap_or(Value::Null)
        }
        Expression::Array(a) => Value::Array(a.iter().map(literal_value).collect()),
        Expression::Object(o) => {
            let mut out = Map::new();
            for (key, value) in o {
                let key = match key {
                    ObjectKey::Ident(i) => i.as_str(),
                    ObjectKey::Expression(Expression::String(s)) => s.value(),
                    _ => return Value::Null,
                };
                out.insert(key.into(), literal_value(value.expr()));
            }
            Value::Object(out)
        }
        Expression::Traversal(t) => {
            // Only a same-module resource identity is useful as a routing scope. It is not
            // the value of an expression, and cannot become a tag or an event field.
            use hcl_edit::expr::TraversalOperator;
            if let Expression::Variable(root) = &t.expr {
                let parts = t
                    .operators
                    .iter()
                    .map(|o| match o.value() {
                        TraversalOperator::GetAttr(a) => Some(a.as_str()),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(parts) = parts.filter(|p| p.len() == 2 && p[1] == "id") {
                    return Value::String(format!("${{{}.{}.id}}", root.as_str(), parts[0]));
                }
            }
            Value::Null
        }
        _ => Value::Null,
    }
}

pub(super) fn json(
    path: &str,
    doc: &Value,
    document: usize,
    spans: &Spans,
    out: &mut Summary,
    bytes: &mut usize,
) {
    for (resource, resource_path) in
        block_paths(doc.get("resource"), vec![Step::Key("resource".into())])
    {
        for (ty, types) in resource.as_object().into_iter().flatten() {
            if !known(ty) {
                continue;
            }
            let mut type_path = resource_path.clone();
            type_path.push(Step::Key(ty.clone()));
            for (resources, type_path) in block_paths(Some(types), type_path) {
                for (name, values) in resources.as_object().into_iter().flatten() {
                    let mut value_path = type_path.clone();
                    value_path.push(Step::Key(name.clone()));
                    for (value, value_path) in block_paths(Some(values), value_path) {
                        let at = |steps: &[Step]| {
                            spans
                                .get(&(document, crate::config_values::key_path(steps)))
                                .copied()
                        };
                        let (line, end) = at(&value_path).unwrap_or((1, 1));
                        let mut locations = BTreeMap::new();
                        let mut set_path = value_path.clone();
                        set_path.push(Step::Key("set".into()));
                        for (s, (set, mut rule_path)) in block_paths(value.get("set"), set_path)
                            .into_iter()
                            .enumerate()
                        {
                            rule_path.push(Step::Key("rule".into()));
                            for (r, (_, rule_path)) in block_paths(set.get("rule"), rule_path)
                                .into_iter()
                                .enumerate()
                            {
                                if let Some(span) = at(&rule_path) {
                                    locations.insert(format!(".set[{s}].rule[{r}]"), span);
                                }
                            }
                        }
                        read(
                            Source {
                                path,
                                line,
                                end_line: end,
                                locations: &locations,
                            },
                            ty,
                            name,
                            value,
                            out,
                            bytes,
                        );
                        if out.truncated {
                            return;
                        }
                    }
                }
            }
        }
    }
}
fn block_paths(value: Option<&Value>, path: Vec<Step>) -> Vec<(&Value, Vec<Step>)> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                let mut path = path.clone();
                path.push(Step::Index(i));
                (value, path)
            })
            .collect(),
        Some(value @ Value::Object(_)) => vec![(value, path)],
        _ => Vec::new(),
    }
}
fn blocks(value: Option<&Value>) -> &[Value] {
    match value {
        Some(Value::Array(values)) => values,
        Some(value @ Value::Object(_)) => std::slice::from_ref(value),
        _ => &[],
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|s| config::literal(s))
        .map(String::from)
}
fn scope(value: &Value, key: &str) -> Option<String> {
    let raw = value.get(key)?;
    if let Some(s) = raw.as_str() {
        if let Some(reference) = s.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
            let parts = reference.split('.').collect::<Vec<_>>();
            let resource = match key {
                "team_id" => "opsgenie_team",
                "service" | "route_to" => "pagerduty_service",
                "event_orchestration" => "pagerduty_event_orchestration",
                _ => return None,
            };
            if parts.len() == 3
                && parts[0] == resource
                && parts[2] == "id"
                && parts[..2].iter().all(|p| {
                    p.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                })
            {
                return Some(parts[..2].join("."));
            }
        }
        if config::literal(s) && !s.is_empty() {
            return Some(s.into());
        }
    }
    None
}
fn block<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let values = blocks(value.get(key));
    (values.len() == 1).then(|| &values[0])
}

fn read(
    source: Source<'_>,
    ty: &str,
    name: &str,
    value: &Value,
    out: &mut Summary,
    bytes: &mut usize,
) {
    let repeated = value.get("count").is_some() || value.get("for_each").is_some();
    if repeated && ty != "opsgenie_team_routing_rule" {
        return;
    }
    let key = format!("{ty}.{name}");
    if ty.starts_with("pagerduty_event_orchestration_") {
        pagerduty(source, ty, &key, value, out, bytes);
        return;
    }
    let Source {
        path,
        line,
        end_line,
        ..
    } = source;
    if ty == "datadog_monitor" {
        let Some(title) = text(value, "name") else {
            return;
        };
        let Some(tags) = value.get("tags") else {
            return;
        };
        let mut unknown_tags = !tags.is_array() || value.get("dynamic").is_some();
        let tags = config::array(Some(tags))
            .iter()
            .filter_map(|v| {
                let Some(v) = v.as_str().filter(|s| config::literal(s)) else {
                    unknown_tags = true;
                    return None;
                };
                Some(v.to_string())
            })
            .collect::<BTreeSet<_>>();
        let rule = Rule {
            key,
            name: title,
            line,
            end_line,
            labels: BTreeMap::new(),
            unknown: BTreeSet::new(),
            tags,
            unknown_tags,
            stream: Stream::Opsgenie,
            scopes: BTreeSet::new(),
        };
        if config::charge_rule(&rule, path, bytes) {
            out.rules.push(rule);
        } else {
            out.truncated = true;
        }
        return;
    }
    let receiver = scope(value, "team_id");
    let criteria = block(value, "criteria");
    let mode = criteria.and_then(|c| text(c, "type"));
    let barrier = repeated || criteria.is_none();
    let mut opaque = criteria.is_some_and(|c| c.get("dynamic").is_some());
    let mut valid = !barrier
        && receiver.is_some()
        && !value.get("dynamic").is_some()
        && matches!(
            mode.as_deref(),
            Some("match-all" | "match-all-conditions" | "match-any-condition")
        );
    let mut matchers = Vec::new();
    for condition in blocks(criteria.and_then(|c| c.get("conditions"))) {
        let matcher = (|| {
            if text(condition, "field")?.as_str() != "tags" {
                return None;
            }
            let op = match text(condition, "operation")?.as_str() {
                "equals" => Op::Equal,
                "matches" => Op::Regex,
                "starts-with" => Op::Prefix,
                "contains" => Op::Contains,
                "ends-with" => Op::Suffix,
                _ => return None,
            };
            let op = match condition.get("not") {
                None | Some(Value::Bool(false)) => op,
                Some(Value::Bool(true)) => op.negated(),
                _ => return None,
            };
            let expected = text(condition, "expected_value")?;
            let (key, value) = config::tag(&expected).unwrap_or(("tags", expected.as_str()));
            Some(Matcher {
                key: key.into(),
                value: value.into(),
                op,
                tags: true,
                field: String::new(),
                tag_value: expected.clone(),
            })
        })();
        if let Some(matcher) = matcher {
            matchers.push(matcher);
        } else {
            opaque = true;
        }
    }
    valid &= !matchers.is_empty() || mode.as_deref() == Some("match-all");
    let route = Route {
        key,
        unknown_scope: receiver.is_none(),
        receiver: receiver.unwrap_or_else(|| "unresolved incident scope".into()),
        line,
        end_line,
        matchers,
        parent: None,
        continuing: false,
        valid,
        stream: Stream::Opsgenie,
        any: mode.as_deref() == Some("match-any-condition"),
        group_by: Vec::new(),
        document: 0,
        order: value
            .get("order")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok()),
        group: path
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent)
            .into(),
        set: String::new(),
        next: None,
        logic: Vec::new(),
        opaque,
        barrier,
    };
    if config::charge(&route, bytes) && config::charge_route(&route, path, &out.routes, bytes) {
        out.routes.push(route);
    } else {
        out.truncated = true;
    }
}

fn pagerduty(
    source: Source<'_>,
    ty: &str,
    key: &str,
    value: &Value,
    out: &mut Summary,
    bytes: &mut usize,
) {
    use super::pcl::{self, Test};
    let Source {
        path,
        line,
        end_line,
        locations,
    } = source;
    let router = ty == "pagerduty_event_orchestration_router";
    let scope_value = scope(
        value,
        if ty == "pagerduty_event_orchestration_service" {
            "service"
        } else {
            "event_orchestration"
        },
    );
    let group = format!("{path}::{key}");
    for (s, set) in blocks(value.get("set")).iter().enumerate() {
        let Some(set_id) = text(set, "id") else {
            continue;
        };
        for (r, rule) in blocks(set.get("rule")).iter().enumerate() {
            if rule.get("disabled").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let mut valid = scope_value.is_some()
                && value.get("dynamic").is_none()
                && set.get("dynamic").is_none()
                && rule.get("dynamic").is_none()
                && rule.get("disabled").is_none_or(Value::is_boolean);
            let mut matchers = Vec::new();
            let mut logic = Vec::new();
            let conditions = blocks(rule.get("condition"));
            for (i, condition) in conditions.iter().enumerate() {
                if let Some((found, tests)) =
                    text(condition, "expression").and_then(|s| pcl::parse(&s))
                {
                    let offset = matchers.len();
                    logic.extend(tests.into_iter().map(|t| match t {
                        Test::Atom(n) => Test::Atom(n + offset),
                        other => other,
                    }));
                    matchers.extend(found);
                } else {
                    logic.push(Test::Unknown);
                }
                if i > 0 {
                    logic.push(Test::Or);
                }
            }
            valid &= rule
                .get("condition")
                .is_none_or(|v| v.is_array() || v.is_object());
            let actions = block(rule, "actions");
            valid &= actions.is_some();
            let target = actions.and_then(|a| scope(a, "route_to"));
            let receiver = if router {
                target.clone()
            } else {
                scope_value.clone()
            };
            valid &= receiver.is_some();
            let next = if router {
                None
            } else {
                target.map(|target| {
                    // Actions that change event data make the downstream input unread.
                    if actions.is_some_and(|a| {
                        ["extraction", "variable", "severity", "event_action"]
                            .iter()
                            .any(|k| a.get(k).is_some())
                    }) {
                        "(transformed event)".into()
                    } else {
                        target
                    }
                })
            };
            let (line, end_line) = locations
                .get(&format!(".set[{s}].rule[{r}]"))
                .copied()
                .unwrap_or((line, end_line));
            let item = Route {
                key: format!("{key}.set[{s}].rule[{r}]"),
                receiver: receiver.unwrap_or_else(|| "unresolved incident scope".into()),
                line,
                end_line,
                matchers,
                parent: None,
                continuing: false,
                valid,
                stream: Stream::Pagerduty,
                any: false,
                group_by: Vec::new(),
                document: 0,
                order: Some(r),
                group: group.clone(),
                set: set_id.clone(),
                next,
                logic,
                opaque: false,
                barrier: false,
                unknown_scope: false,
            };
            if config::charge(&item, bytes) && config::charge_route(&item, path, &out.routes, bytes)
            {
                out.routes.push(item);
            } else {
                out.truncated = true;
                return;
            }
        }
    }
}
