//! Literal Ingress prefixes, kept with evaluated Service addresses for the HTTP kind.

use super::{OBJECT_REFERENCES_MAX, UNIT_STORED_MAX, list, plain, text};
use crate::iac::Ingress;
use serde_json::Value;

pub(super) fn read(value: &Value) -> Vec<Ingress> {
    // An unread rewrite is an unread mount, including annotations of other controllers.
    let rewrite = value["metadata"]["annotations"]
        .as_object()
        .is_some_and(|annotations| {
            annotations
                .keys()
                .any(|key| key.to_ascii_lowercase().contains("rewrite"))
        });
    let mut routes = Vec::<Ingress>::new();
    let mut remaining = UNIT_STORED_MAX;
    for rule in list(value, &["spec", "rules"]) {
        let Some(host) = text(rule, &["host"])
            .filter(|host| plain(host) && !host.is_empty() && !host.contains('*'))
        else {
            continue;
        };
        for path in list(rule, &["http", "paths"]) {
            let Some(prefix) = text(path, &["path"]) else {
                continue;
            };
            if !prefix.starts_with('/')
                || !plain(prefix)
                || prefix
                    .bytes()
                    .any(|b| !b.is_ascii_alphanumeric() && !b"/-._~".contains(&b))
            {
                continue;
            }
            let service = text(path, &["backend", "service", "name"])
                .or_else(|| text(path, &["backend", "serviceName"]))
                .filter(|name| plain(name))
                .filter(|_| {
                    !rewrite && text(path, &["pathType"]).is_none_or(|kind| kind == "Prefix")
                })
                .unwrap_or_default();
            // Charge borrowed text before repeating a host for each path or cloning an alias.
            let size = host
                .len()
                .saturating_add(prefix.len())
                .saturating_add(service.len());
            let next = remaining.checked_sub(size);
            if routes.len() == OBJECT_REFERENCES_MAX || next.is_none() {
                // No partial prefix table may turn an unread longer rule into a shorter route.
                for route in &mut routes {
                    route.prefix.clear();
                    route.service.clear();
                }
                routes.sort_by(|a, b| a.host.cmp(&b.host));
                routes.dedup_by(|a, b| a.host == b.host);
                return routes;
            }
            remaining = next.unwrap_or_default();
            routes.push(Ingress {
                host: host.to_ascii_lowercase(),
                prefix: prefix.trim_end_matches('/').into(),
                service: service.into(),
            });
        }
    }
    routes
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn a_truncated_ingress_table_keeps_no_partial_route() {
        for (host, count) in [
            ("shop.example.com".to_string(), 65),
            ("h".repeat(70_000), 64),
        ] {
            let paths = (0..count)
                .map(|i| {
                    json!({
                        "path": format!("/store/{i}"), "pathType": "Prefix",
                        "backend": {"service": {"name": "orders"}}
                    })
                })
                .collect::<Vec<_>>();
            let value = json!({"spec": {"rules": [{"host": host, "http": {"paths": paths}}]}});
            let routes = super::read(&value);
            assert!(!routes.is_empty());
            assert!(
                routes
                    .iter()
                    .all(|r| r.service.is_empty() && r.prefix.is_empty())
            );
        }
    }
}
