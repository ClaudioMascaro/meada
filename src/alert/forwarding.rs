//! Known Alertmanager receiver mappings. Templates are substituted once, never executed.
use super::config::{self, Route, Rule, Stream, Summary};
use super::{Limit, Work};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize)]
pub(super) struct Receiver {
    name: String,
    document: usize,
    stream: Stream,
    tags: Option<String>,
    details: BTreeMap<String, String>,
    scopes: BTreeSet<String>,
    valid: bool,
}

pub(super) fn read(body: &Value, document: usize, out: &mut Summary, bytes: &mut usize) {
    for receiver in config::array(body.get("receivers")) {
        let Some(name) = receiver
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| config::literal(s))
        else {
            continue;
        };
        for (snake, camel, stream) in [
            ("opsgenie_configs", "opsgenieConfigs", Stream::Opsgenie),
            ("pagerduty_configs", "pagerdutyConfigs", Stream::Pagerduty),
        ] {
            for value in config::array(receiver.get(snake).or_else(|| receiver.get(camel))) {
                let tags = value.get("tags").and_then(Value::as_str).map(String::from);
                let mut valid = value.is_object();
                let mut details = BTreeMap::new();
                if stream == Stream::Pagerduty {
                    // Alertmanager's Events API v2 writes these details to custom_details.
                    // The service_key (v1) receiver has a different payload, so it is not this path.
                    valid &= value
                        .get("routing_key")
                        .or_else(|| value.get("routingKey"))
                        .is_some_and(|v| match v {
                            Value::String(s) => !s.is_empty(),
                            Value::Object(o) => !o.is_empty(),
                            _ => false,
                        })
                        || value
                            .get("routing_key_file")
                            .and_then(Value::as_str)
                            .is_some_and(|v| !v.is_empty());
                    if let Some(map) = value.get("details").and_then(Value::as_object) {
                        for (key, template) in map {
                            if let Some(template) = template.as_str() {
                                details.insert(key.clone(), template.into());
                            }
                        }
                    }
                    // AlertmanagerConfig represents details as [{key, value}], unlike YAML.
                    for detail in config::array(value.get("details")) {
                        if let (Some(key), Some(template)) = (
                            detail.get("key").and_then(Value::as_str),
                            detail.get("value").and_then(Value::as_str),
                        ) {
                            details.insert(key.into(), template.into());
                        }
                    }
                }
                let mut scopes = BTreeSet::new();
                if let Some(responders) = value.get("responders") {
                    valid &= responders.is_array();
                    for responder in config::array(Some(responders)) {
                        if responder.get("type").and_then(Value::as_str) != Some("team") {
                            valid = false;
                            continue;
                        }
                        if let Some(id) = responder
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty() && config::literal(s))
                        {
                            scopes.insert(id.into());
                        } else {
                            // A name alone is not the literal team ID tested by team_id.
                            valid = false;
                        }
                    }
                }
                let item = Receiver {
                    name: name.into(),
                    document,
                    stream,
                    tags,
                    details,
                    scopes,
                    valid,
                };
                if config::charge(&item, bytes) {
                    out.forwardings.push(item);
                } else {
                    out.truncated = true;
                    return;
                }
            }
        }
    }
}

pub(super) struct Event {
    pub rule: Rule,
    pub reason: String,
}

pub(super) fn events(
    summary: &Summary,
    route: &Route,
    rule: &Rule,
    at: &str,
    retained: &mut usize,
    work: &mut Work,
) -> Result<Vec<Event>, Limit> {
    let mut out = Vec::new();
    for receiver in &summary.forwardings {
        work.spend(1)?;
        if !receiver.valid || receiver.name != route.receiver || receiver.document != route.document
        {
            continue;
        }
        reserve(
            retained,
            receiver.scopes.iter().map(String::len).sum::<usize>() + std::mem::size_of::<Rule>(),
        )?;
        let mut payload = Rule {
            key: String::new(),
            name: String::new(),
            line: 0,
            end_line: 0,
            labels: BTreeMap::new(),
            unknown: BTreeSet::new(),
            tags: BTreeSet::new(),
            unknown_tags: false,
            stream: receiver.stream,
            scopes: receiver.scopes.clone(),
        };
        let mut present = false;
        if receiver.stream == Stream::Opsgenie {
            if let Some(template) = &receiver.tags {
                if let Some(tags) = render(template, route, rule, retained, work)? {
                    // Plain tags can affect a conjunction, even though only key:value tags
                    // declare emitter contracts. Keep the complete native payload.
                    let tags = tags.trim();
                    for tag in tags.split(',').filter(|_| !tags.is_empty()) {
                        reserve(retained, tag.len())?;
                        payload.tags.insert(tag.into());
                        present = true;
                    }
                } else {
                    payload.unknown_tags = true;
                }
            }
        } else {
            for (key, template) in &receiver.details {
                // A literal detail key with a dot is one JSON key, not a nested PCL path.
                if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    continue;
                }
                reserve(retained, "event.custom_details.".len() + key.len())?;
                let field = format!("event.custom_details.{key}");
                if let Some(value) = render(template, route, rule, retained, work)? {
                    payload.labels.insert(field, value);
                    present = true;
                } else {
                    payload.unknown.insert(field);
                }
            }
        }
        if !present {
            continue;
        }
        let suffix = match receiver.stream {
            Stream::Opsgenie => "tags",
            _ => "details -> event.custom_details",
        };
        reserve(
            retained,
            at.len() + receiver.name.len() + suffix.len() + 64 + std::mem::size_of::<Event>(),
        )?;
        let reason = format!(
            "forwarding: Alertmanager {at}; receiver: {}; {suffix}",
            receiver.name
        );
        out.push(Event {
            rule: payload,
            reason,
        });
    }
    Ok(out)
}

pub(super) fn reserve(retained: &mut usize, bytes: usize) -> Result<(), Limit> {
    *retained = retained.checked_add(bytes).ok_or(Limit::Output)?;
    if *retained > super::ALERT_LINK_BYTES_MAX {
        return Err(Limit::Output);
    }
    Ok(())
}

fn render(
    template: &str,
    route: &Route,
    rule: &Rule,
    retained: &mut usize,
    work: &mut Work,
) -> Result<Option<String>, Limit> {
    // Unread templates may emit nothing. Charge their scan too, before repeated rules can
    // spend unbounded time on the same long, unsupported expression.
    work.spend(template.len())?;
    let mut out = String::new();
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        let prefix = &remaining[..start];
        let Some(end) = remaining[start + 2..].find("}}") else {
            return Ok(None);
        };
        let expression = remaining[start + 2..start + 2 + end].trim();
        let Some(key) = expression
            .strip_prefix(".CommonLabels.")
            .or_else(|| expression.strip_prefix(".GroupLabels."))
        else {
            return Ok(None);
        };
        if key.is_empty()
            || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || !route
                .group_by
                .iter()
                .any(|group| group == key || group == "...")
            || rule.unknown.contains(key)
        {
            return Ok(None);
        }
        let Some(value) = rule.labels.get(key) else {
            return Ok(None);
        };
        reserve(retained, prefix.len() + value.len())?;
        out.push_str(prefix);
        out.push_str(value);
        remaining = &remaining[start + 2 + end + 2..];
    }
    if remaining.contains("}}") || remaining.contains("${") || remaining.contains("%{") {
        return Ok(None);
    }
    reserve(retained, remaining.len())?;
    out.push_str(remaining);
    Ok(Some(out))
}
