//! Literal Ingress prefixes, kept with evaluated Service addresses for the HTTP kind.

use super::{OBJECT_REFERENCES_MAX, UNIT_STORED_MAX, list, plain, text};
use crate::iac::Ingress;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) fn read(value: &Value) -> Vec<Ingress> {
    // An unread rewrite or regular expression is an unread mount, including annotations of other
    // controllers.
    let unread = value["metadata"]["annotations"]
        .as_object()
        .is_some_and(|annotations| {
            annotations.keys().any(|key| {
                let key = key.to_ascii_lowercase();
                key.contains("rewrite") || key.contains("use-regex")
            })
        });
    fn host_of(rule: &Value) -> Option<&str> {
        text(rule, &["host"]).filter(|host| plain(host) && !host.is_empty() && !host.contains('*'))
    }
    let literal = |b: u8| b.is_ascii_alphanumeric() || b"/-._~".contains(&b);
    let mut routes = Vec::<Ingress>::new();
    let mut remaining = UNIT_STORED_MAX;
    for rule in list(value, &["spec", "rules"]) {
        let Some(host) = host_of(rule) else {
            continue;
        };
        for path in list(rule, &["http", "paths"]) {
            let Some(written) = text(path, &["path"]) else {
                continue;
            };
            let read = written.starts_with('/') && written.bytes().all(literal);
            // A path read as no literal prefix is an unknown mount at its literal head, cut at
            // its last slash, so the URLs it may serve fall back to no shorter route.
            let prefix = if read {
                written.trim_end_matches('/')
            } else {
                let head = &written[..written.bytes().position(|b| !literal(b)).unwrap_or(0)];
                head.rfind('/').map_or("", |at| &head[..at])
            };
            let service = text(path, &["backend", "service", "name"])
                .or_else(|| text(path, &["backend", "serviceName"]))
                .filter(|name| plain(name))
                .filter(|_| read && !unread && text(path, &["pathType"]) == Some("Prefix"))
                .unwrap_or_default();
            // Charge borrowed text before repeating a host for each path or cloning an alias.
            let size = host
                .len()
                .saturating_add(prefix.len())
                .saturating_add(service.len());
            let next = remaining.checked_sub(size);
            if routes.len() == OBJECT_REFERENCES_MAX || next.is_none() {
                // No partial prefix table may turn an unread longer rule into a shorter route:
                // every host of the Ingress is one unknown mount.
                let hosts = (list(value, &["spec", "rules"]).filter_map(host_of))
                    .map(str::to_ascii_lowercase)
                    .collect::<BTreeSet<_>>();
                return (hosts.into_iter())
                    .map(|host| Ingress {
                        host,
                        prefix: String::new(),
                        service: String::new(),
                    })
                    .collect();
            }
            remaining = next.unwrap_or_default();
            routes.push(Ingress {
                host: host.to_ascii_lowercase(),
                prefix: prefix.into(),
                service: service.into(),
            });
        }
    }
    routes
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// A path the reader cannot take as a literal prefix is an unknown mount at its literal head,
    /// cut at its last slash, so the URLs it may serve fall back to no other route; a path with
    /// no `pathType`, or under a `use-regex` annotation, is an unknown mount at its own prefix,
    /// since the controller reads it as it likes.
    #[test]
    fn an_unread_path_is_an_unknown_mount_at_its_literal_head() {
        let routes = |annotations: serde_json::Value, paths: serde_json::Value| {
            let value = json!({"metadata": {"annotations": annotations}, "spec": {"rules": [
                {"host": "shop.example.com", "http": {"paths": paths}}]}});
            (super::read(&value).into_iter())
                .map(|r| (r.prefix, r.service))
                .collect::<Vec<_>>()
        };
        let path = |path: &str, kind: Option<&str>, service: &str| {
            let mut path = json!({"path": path, "backend": {"service": {"name": service}}});
            if let Some(kind) = kind {
                path["pathType"] = json!(kind);
            }
            path
        };
        let paths = json!([
            path("/store", Some("Prefix"), "orders"),
            path("/store/(.*)", Some("Prefix"), "cart"),
            path("/api/*", Some("ImplementationSpecific"), "api"),
            path("/v1(.*)", Some("Prefix"), "v1"),
            path("/legacy", None, "old"),
        ]);
        let pair = |prefix: &str, service: &str| (prefix.to_string(), service.to_string());
        assert_eq!(
            routes(json!({}), paths.clone()),
            [
                pair("/store", "orders"),
                pair("/store", ""),
                pair("/api", ""),
                pair("", ""),
                pair("/legacy", ""),
            ]
        );
        let regex = json!({"nginx.ingress.kubernetes.io/use-regex": "true"});
        assert_eq!(
            routes(regex, json!([path("/store", Some("Prefix"), "orders")])),
            [pair("/store", "")]
        );
    }

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
            // A host whose rules come after the cut is no more read than the cut one.
            let later = json!({"host": "later.example.com", "http": {"paths": [
                {"path": "/store", "pathType": "Prefix", "backend": {"service": {"name": "cart"}}}]}});
            let value =
                json!({"spec": {"rules": [{"host": host, "http": {"paths": paths}}, later]}});
            let routes = super::read(&value);
            assert!(routes.iter().any(|r| r.host == "later.example.com"));
            assert!(
                routes
                    .iter()
                    .all(|r| r.service.is_empty() && r.prefix.is_empty())
            );
        }
    }
}
