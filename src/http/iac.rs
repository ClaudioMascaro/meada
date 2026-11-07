//! IaC providers use evaluated route facts and indexing-time validated handler modules.

use crate::contract::ConfigFacts;
use crate::index::{Export, Provider};
use crate::workspace::Repository;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

// ponytail: repeated route locations and handler names must not multiply a file's stored facts.
const IAC_HTTP_STORED_MAX: usize = 4 * crate::lang::MAX_BYTES;

#[derive(Serialize, Deserialize)]
struct Route {
    contract: String,
    handler: Option<(String, String)>,
    inferred: bool,
}

pub(super) fn is_file(path: &str) -> bool {
    crate::iac::is_source(path) || crate::iac::cloudformation::is_source(path)
}

pub(super) fn summary(
    _: &str,
    src: &[u8],
    root: &Path,
    facts: &ConfigFacts<'_>,
) -> serde_json::Value {
    let mut routes = Vec::new();
    let mut room = IAC_HTTP_STORED_MAX;
    if src.len() > crate::lang::MAX_BYTES {
        return serde_json::Value::Null;
    }
    for node in facts.nodes {
        let handler = crate::image::code::checked_handler(root, node, room);
        for route in &node.routes {
            let method = route.method.text.to_uppercase();
            let path = &route.path.text;
            if (!super::METHODS.contains(&method.as_str()) && method != super::ANY)
                || !path.starts_with('/')
                || path.chars().any(char::is_whitespace)
                || path.contains(['$', '*', '?', '#'])
                || !super::balanced(path)
            {
                continue;
            }
            let contract = super::contract(&method, path);
            let bytes = contract.len() + handler.as_ref().map_or(0, |(p, n)| p.len() + n.len());
            let Some(left) = room.checked_sub(bytes) else {
                return serde_json::Value::Null;
            };
            room = left;
            routes.push(Route {
                contract,
                handler: handler.clone(),
                inferred: route.method.overridable || route.path.overridable,
            });
        }
    }
    if serde_json::to_vec(&routes).map_or(true, |bytes| bytes.len() > IAC_HTTP_STORED_MAX) {
        return serde_json::Value::Null;
    }
    serde_json::to_value(routes).unwrap_or_default()
}

fn routes(export: &Export) -> Vec<Route> {
    export
        .configurations
        .iter()
        .filter_map(|(_, summary)| summary.get("http"))
        .filter_map(|summary| serde_json::from_value::<Vec<Route>>(summary.clone()).ok())
        .flatten()
        .collect()
}

pub(super) fn read(db: &rusqlite::Connection, export: &mut Export) -> crate::Result<()> {
    let paths: HashSet<_> = routes(export)
        .into_iter()
        .filter_map(|r| r.handler.map(|h| h.0))
        .collect();
    export.exported.extend(crate::index::exports_of(
        db,
        &paths.into_iter().collect::<Vec<_>>(),
    )?);
    export.exported.sort_by(|a, b| {
        (&a.path, &a.name, &a.file, &a.target).cmp(&(&b.path, &b.name, &b.file, &b.target))
    });
    export.exported.dedup();
    Ok(())
}

pub(super) fn symbols(repo: &Repository, export: &Export) -> Vec<String> {
    let aliases = aliases(&export.exported);
    routes(export)
        .into_iter()
        .filter_map(|r| r.handler)
        .filter_map(|handler| handler_id(repo, &handler, &aliases))
        .collect()
}

type Aliases<'a> = HashMap<(&'a str, &'a str), Option<(&'a str, &'a str)>>;

/// Stored exact exports resolve aliases/re-exports; conflicting or opaque exports stay open.
fn aliases(exported: &[crate::resolve::Exported]) -> Aliases<'_> {
    let mut aliases = HashMap::new();
    for row in exported {
        let Some(name) = row.name.as_deref() else {
            continue;
        };
        let target = row.file.as_deref().zip(row.target.as_deref());
        aliases
            .entry((row.path.as_str(), name))
            .and_modify(|prior| {
                if *prior != target {
                    *prior = None;
                }
            })
            .or_insert(target);
    }
    aliases
}

fn handler_id(
    repo: &Repository,
    (path, name): &(String, String),
    aliases: &Aliases<'_>,
) -> Option<String> {
    let (path, name) = match aliases.get(&(path.as_str(), name.as_str())) {
        Some(target) => (*target)?,
        None => (path.as_str(), name.as_str()),
    };
    Some(format!("{}::{path}::{name}", repo.identity))
}

pub(super) fn declare(repo: &Repository, export: &mut Export) {
    let routes = routes(export);
    if routes.is_empty() {
        return;
    }
    let aliases = aliases(&export.exported);
    let symbols: HashSet<_> = export.symbols.iter().map(|s| s.id.as_str()).collect();
    let mut provided: HashMap<_, _> = export
        .providers
        .iter()
        .enumerate()
        .map(|(index, p)| ((p.contract.clone(), p.site.clone()), index))
        .collect();
    for route in routes {
        let site = route
            .handler
            .and_then(|handler| handler_id(repo, &handler, &aliases))
            .filter(|id| symbols.contains(id.as_str()))
            .unwrap_or_else(|| repo.identity.clone());
        let key = (route.contract.clone(), site.clone());
        if let Some(&index) = provided.get(&key) {
            if !route.inferred {
                export.providers[index].evidence = None;
            }
            continue;
        }
        provided.insert(key, export.providers.len());
        export.providers.push(Provider {
            contract: route.contract,
            site,
            evidence: route
                .inferred
                .then(|| "IaC route through an overridable value".into()),
            ..Provider::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_summary_drops_the_whole_summary_past_its_stored_ceiling() {
        let route = crate::iac::Route {
            method: crate::iac::Resolved {
                text: "GET".into(),
                overridable: false,
            },
            path: crate::iac::Resolved {
                text: format!("/{}", "x".repeat(1024)),
                overridable: false,
            },
            at: "template.yaml#Resources.Function.Events.Route".into(),
        };
        let node = crate::iac::Node {
            routes: vec![route; IAC_HTTP_STORED_MAX / 1024 + 1],
            ..Default::default()
        };
        assert!(
            summary(
                "main.tf",
                b"",
                Path::new("."),
                &ConfigFacts { nodes: &[&node] }
            )
            .is_null()
        );
    }
}
