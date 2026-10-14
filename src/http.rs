//! The HTTP contract kind: routes served by providers and the `fetch` calls that consume them.
//! A contract is `http::<METHOD>::<normalized path>`.

use crate::contract::{ContractKind, Found, Level, Link, Site};
use crate::index::{Consumer, Provider};
use crate::lang::{Export, Facts, Part, Value};
use crate::workspace::Member;

/// What every parameter (`[id]`, `[...slug]`, `:id`, a template hole) becomes in a path.
pub const PARAM: &str = "{}";

pub struct Http;

/// The contract identifier of `method` on `path`.
pub fn contract(method: &str, path: &str) -> String {
    format!("http::{}::{}", method.to_uppercase(), normalize_path(path))
}

/// Drops the query string, the fragment and a trailing `/`, and turns every segment holding
/// a parameter into `PARAM`. Case and API versions are kept. Template holes must already be
/// written as `PARAM`.
pub fn normalize_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segments: Vec<&str> = path
        .split('/')
        .map(|s| {
            let param =
                s.contains(PARAM) || s.starts_with(':') || (s.starts_with('[') && s.ends_with(']'));
            if param { PARAM } else { s }
        })
        .collect();
    let path = segments.join("/");
    let path = path.trim_end_matches('/');
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"];

/// The route a Next.js app-router file serves: `app/users/[id]/route.ts` (or under
/// `src/app/`) serves `/users/{}`. Route groups `(name)` are dropped.
fn next_route(path: &str) -> Option<String> {
    let rest = path
        .strip_prefix("src/")
        .unwrap_or(path)
        .strip_prefix("app/")?;
    let (dir, file) = rest.rsplit_once('/').unwrap_or(("", rest));
    if !matches!(file, "route.ts" | "route.js") {
        return None;
    }
    let segments: Vec<&str> = dir
        .split('/')
        .filter(|s| !s.is_empty() && !(s.starts_with('(') && s.ends_with(')')))
        .collect();
    Some(format!("/{}", segments.join("/")))
}

/// The contract and `via` of `fetch(url, init)`; the contract is `None` when the URL or the
/// method cannot be read at the call.
fn fetch(args: &[Value]) -> (Option<String>, Option<String>) {
    let method = match args.get(1) {
        None => Some("GET".to_string()),
        // The last `method` wins, as in JavaScript; a spread may hide one.
        Some(Value::Object(properties)) => {
            match properties
                .iter()
                .rfind(|(k, _)| k == "method" || k == "...")
            {
                None => Some("GET".to_string()),
                Some((_, Value::Str(parts))) => literal(parts),
                Some(_) => None,
            }
        }
        Some(_) => None,
    };
    let Some(Value::Str(parts)) = args.first() else {
        return (None, None);
    };
    let (via, path) = match parts.split_first() {
        Some((Part::Env(var), rest)) => (Some(format!("env:{var}")), Some(render(rest))),
        _ => {
            let url = render(parts);
            match url.split_once("://") {
                Some((_, rest)) => {
                    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                    let host = &rest[..end];
                    if host.contains(PARAM) || host.is_empty() {
                        (None, None)
                    } else {
                        let via = format!("host:{}", host.to_lowercase());
                        (Some(via), Some(rest[end..].to_string()))
                    }
                }
                None => (None, Some(url)),
            }
        }
    };
    // A path that does not start at the root, such as `${base}${path}`, is not read.
    let path = path.filter(|p| p.is_empty() || p.starts_with(['/', '?', '#']));
    match (method, path) {
        (Some(method), Some(path)) => (Some(contract(&method, &path)), via),
        _ => (None, via),
    }
}

/// The text of `parts` when every part is literal.
fn literal(parts: &[Part]) -> Option<String> {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// `parts` as a path, every non-literal part a parameter.
fn render(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => t.as_str(),
            Part::Env(_) | Part::Hole => PARAM,
        })
        .collect()
}

impl ContractKind for Http {
    fn prefix(&self) -> &'static str {
        "http"
    }

    fn extract(&self, path: &str, facts: &Facts) -> Vec<Found> {
        let mut found: Vec<Found> = facts
            .calls
            .iter()
            .enumerate()
            .filter(|(_, call)| call.callee == "fetch" && !call.args.is_empty())
            .map(|(n, call)| {
                let (contract, via) = fetch(&call.args);
                Found {
                    site: Site::Call(n),
                    contract,
                    via,
                }
            })
            .collect();
        if let Some(route) = next_route(path) {
            for export in &facts.exports {
                let Export::Local { exported, local } = export else {
                    continue;
                };
                if !METHODS.contains(&exported.as_str()) {
                    continue;
                }
                // `export const GET = withAuth(...)` defines no function: the file serves it.
                let site = if facts.defs.iter().any(|d| d.name == *local) {
                    Site::Def(local.clone())
                } else {
                    Site::File
                };
                found.push(Found {
                    site,
                    contract: Some(contract(exported, &route)),
                    via: None,
                });
            }
        }
        found
    }

    fn link(
        &self,
        consumer: &Consumer,
        from: &Member,
        providers: &[(&Member, &Provider)],
    ) -> Vec<Link> {
        let (Some(contract), Some(via)) = (&consumer.contract, &consumer.via) else {
            return Vec::new();
        };
        let serving = providers.iter().filter(|(_, p)| p.contract == *contract);
        let (fits, level, reason): (Vec<_>, _, _) = if let Some(host) = via.strip_prefix("host:") {
            let fits = serving
                .filter(|(m, _)| {
                    m.config
                        .serve
                        .hosts
                        .iter()
                        .any(|h| h.eq_ignore_ascii_case(host))
                })
                .collect();
            (fits, Level::Exact, format!("host {host} is served by"))
        } else if let Some(var) = via.strip_prefix("env:") {
            let peers: Vec<&str> = from
                .config
                .peers
                .iter()
                .filter(|p| p.base_url_env.iter().any(|v| v == var))
                .map(|p| p.repo.as_str())
                .collect();
            let fits = serving
                .filter(|(m, _)| peers.contains(&m.identity.as_str()))
                .collect();
            (
                fits,
                Level::Inferred,
                format!("base URL from {var}, a base_url_env of peer"),
            )
        } else {
            return Vec::new();
        };
        let ambiguous = fits.len() > 1;
        fits.iter()
            .map(|(member, provider)| Link {
                consumer: consumer.site.clone(),
                provider: provider.site.clone(),
                contract: contract.clone(),
                level: if ambiguous { Level::Ambiguous } else { level },
                reason: if ambiguous {
                    format!("{} providers fit; {reason} {}", fits.len(), member.identity)
                } else {
                    format!("{reason} {}", member.identity)
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_path;

    #[test]
    fn paths_normalize_alike_on_both_sides() {
        for (path, normalized) in [
            ("/api/users", "/api/users"),
            ("/api/users/", "/api/users"),
            ("/api/users?page=2", "/api/users"),
            ("/api/users#top", "/api/users"),
            ("/api/users/?page=2#top", "/api/users"),
            ("/api/users/[id]", "/api/users/{}"),
            ("/api/users/:id/posts", "/api/users/{}/posts"),
            ("/api/users/{}", "/api/users/{}"),
            ("/api/users/u-{}", "/api/users/{}"),
            ("/docs/[...slug]", "/docs/{}"),
            ("/docs/[[...slug]]", "/docs/{}"),
            ("/api/v1/Users", "/api/v1/Users"),
            ("/api/v2/users", "/api/v2/users"),
            ("/", "/"),
            ("", "/"),
            ("?q=1", "/"),
        ] {
            assert_eq!(normalize_path(path), normalized, "{path}");
        }
    }
}
