//! HTTP routing through evaluated Services, Ingresses and the workloads running code.

use super::{Http, PROXY_VIA, contract, fits, gateway_contracts, join, route};
use crate::contract::{ContractKind, Level, Link};
use crate::iac::Node;
use crate::image::{Image, environment::Environments};
use crate::index::{Consumer, Export, Gateway, Provider};
use crate::workspace::Repository;
use meada_core::export::Execution;
use std::collections::{BTreeMap, BTreeSet};

type LinkKey = (String, String, String);
type Aliases = BTreeMap<(String, String), Vec<Gateway>>;
/// The expanded text of internal Ingress gateway forms per query; overflow makes all unread.
// ponytail: sixteen source-file ceilings cover ordinary route tables; fan-out must not
// multiply a hostile long route by every Ingress rule without bound.
const ALIAS_BYTES_MAX: usize = 16 << 20;

/// Prepared once per query. Presentation shares routing's structured resolution decision.
#[derive(Clone, Default)]
pub(crate) struct Prepared {
    pub(super) links: Vec<Link>,
    explained: BTreeSet<LinkKey>,
}

impl Prepared {
    pub(super) fn explains(&self, link: &Link) -> bool {
        self.explained.contains(&key(link))
    }
}

fn key(link: &Link) -> LinkKey {
    (
        link.consumer.clone(),
        link.provider.clone(),
        link.contract.clone(),
    )
}

/// Kubernetes DNS forms, with an optional numeric port. Other domains remain ordinary hosts.
fn service_name(host: &str) -> Option<(&str, Option<&str>)> {
    let host = hostname(host)?;
    let label = |s: &str| {
        !s.is_empty()
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !s.starts_with('-')
            && !s.ends_with('-')
    };
    let parts = host.split('.').collect::<Vec<_>>();
    match parts.as_slice() {
        [service] if label(service) => Some((service, None)),
        [service, namespace] if label(service) && label(namespace) => {
            Some((service, Some(namespace)))
        }
        [service, namespace, "svc"] if label(service) && label(namespace) => {
            Some((service, Some(namespace)))
        }
        [service, namespace, "svc", "cluster", "local"] if label(service) && label(namespace) => {
            Some((service, Some(namespace)))
        }
        _ => None,
    }
}

fn hostname(host: &str) -> Option<&str> {
    let host = match host.rsplit_once(':') {
        Some((host, port)) => {
            port.parse::<u16>().ok()?;
            host
        }
        None => host,
    };
    Some(host.strip_suffix('.').unwrap_or(host))
}

fn namespace(node: &Node) -> Option<&str> {
    let (_, rest) = node.address.split_once('/')?;
    Some(
        rest.split_once('/')
            .map_or("default", |(namespace, _)| namespace),
    )
}

fn inside(site: &str, repository: &str, path: &str) -> bool {
    site.strip_prefix(repository)
        .and_then(|s| s.strip_prefix("::"))
        .and_then(|s| s.split("::").next())
        .is_some_and(|file| {
            path == "."
                || file == path
                || file.strip_prefix(path).is_some_and(|p| p.starts_with('/'))
        })
}

struct Location<'a> {
    export: &'a Export,
    node: &'a Node,
}

struct Routing<'a> {
    exports: &'a [Export],
    ingress_ready: bool,
    providers: Vec<(&'a Repository, &'a Provider)>,
    executions: Vec<Execution>,
    nodes: BTreeMap<(String, String), Location<'a>>,
    selected: BTreeMap<(&'a str, &'a str, &'a str), Vec<&'a Node>>,
}

impl<'a> Routing<'a> {
    fn new(exports: &'a [Export], repositories: &'a [Repository]) -> Self {
        let providers = exports
            .iter()
            .zip(repositories)
            .flat_map(|(e, r)| e.providers.iter().map(move |p| (r, p)))
            .collect::<Vec<_>>();
        let links = crate::image::execution_links(exports, repositories);
        let executions = Image
            .executions(exports, &links)
            .into_iter()
            .filter(|x| x.level != Level::Ambiguous)
            .collect();
        let nodes = exports
            .iter()
            .flat_map(|export| {
                export.iac.iter().map(move |node| {
                    (
                        (node.identity(&export.repository), node.root.clone()),
                        Location { export, node },
                    )
                })
            })
            .collect();
        let mut selected = BTreeMap::<_, Vec<_>>::new();
        for export in exports {
            let nodes = export
                .iac
                .iter()
                .map(|n| ((n.root.as_str(), n.address.as_str()), n))
                .collect::<BTreeMap<_, _>>();
            for reference in &export.iac_references {
                if reference
                    .via
                    .as_deref()
                    .is_some_and(|v| v.starts_with("selector "))
                    && let Some(node) =
                        nodes.get(&(reference.root.as_str(), reference.target.as_str()))
                {
                    selected
                        .entry((
                            export.repository.as_str(),
                            reference.root.as_str(),
                            reference.source.as_str(),
                        ))
                        .or_default()
                        .push(*node);
                }
            }
        }
        Self {
            exports,
            ingress_ready: true,
            providers,
            executions,
            nodes,
            selected,
        }
    }

    /// Provider forms and matching share one complete, bounded prefix table.
    fn aliases(&self) -> Option<Aliases> {
        let mut aliases = BTreeMap::<_, Vec<_>>::new();
        let mut room = ALIAS_BYTES_MAX;
        for export in self.exports {
            for node in &export.iac {
                for ingress in &node.ingress {
                    let Some(service) = export.iac.iter().find(|s| {
                        s.root == node.root && s.address == ingress.service && s.ty == "Service"
                    }) else {
                        continue;
                    };
                    let selected = self.selected.get(&(
                        export.repository.as_str(),
                        service.root.as_str(),
                        service.address.as_str(),
                    ));
                    for workload in selected.into_iter().flatten() {
                        for execution in self.running(export, workload) {
                            for (repo, provider) in &self.providers {
                                if repo.identity == execution.repository
                                    && inside(&provider.site, &repo.identity, &execution.path)
                                    && let Some((_, path)) = route(&provider.contract)
                                {
                                    // Include keys and reason text before cloning them or joining a path.
                                    let bytes = provider.site.len()
                                        + provider.contract.len()
                                        + path.len()
                                        + 2 * ingress.prefix.len()
                                        + export.repository.len()
                                        + node.path.len()
                                        + node.address.len()
                                        + 32;
                                    room = room.checked_sub(bytes)?;
                                    aliases
                                        .entry((provider.site.clone(), provider.contract.clone()))
                                        .or_default()
                                        .push(Gateway {
                                            path: join(&ingress.prefix, path),
                                            reason: format!(
                                                "Ingress {} prefix {}",
                                                node.identity(&export.repository),
                                                ingress.prefix
                                            ),
                                        });
                                }
                            }
                        }
                    }
                }
            }
        }
        Some(aliases)
    }

    fn location(&self, workload: &str, root: &str) -> Option<&Location<'a>> {
        self.nodes.get(&(workload.into(), root.into()))
    }

    fn callers(&self, repository: &str, site: &str) -> Vec<&Location<'a>> {
        self.executions
            .iter()
            .filter(|x| x.repository == repository && inside(site, repository, &x.path))
            .filter_map(|x| self.location(&x.workload, &x.root))
            .collect()
    }

    fn running(&self, export: &Export, workload: &Node) -> Vec<&Execution> {
        let identity = workload.identity(&export.repository);
        self.executions
            .iter()
            .filter(|x| x.workload == identity && x.root == workload.root)
            .collect()
    }

    fn services(
        &self,
        host: &str,
        caller: Option<&Location>,
        from: &Repository,
    ) -> Vec<(&Export, &Node)> {
        let dns = service_name(host);
        let Some(compose_name) = hostname(host) else {
            return Vec::new();
        };
        self.exports
            .iter()
            .flat_map(|export| export.iac.iter().map(move |node| (export, node)))
            .filter(|(export, node)| {
                if node.ty == "Service" && node.address.starts_with("Service/") {
                    let Some((name, wanted_namespace)) = dns else {
                        return false;
                    };
                    node.name.as_deref() == Some(name)
                        && wanted_namespace.is_none_or(|ns| namespace(node) == Some(ns))
                        && caller.is_none_or(|c| {
                            c.node.ty != "service"
                                && c.export.repository == export.repository
                                && c.node.root == node.root
                                && (wanted_namespace.is_some()
                                    || namespace(c.node) == namespace(node))
                        })
                } else if node.ty == "service"
                    && node
                        .address
                        .strip_prefix("services.")
                        .is_some_and(|name| name.eq_ignore_ascii_case(compose_name))
                {
                    caller.map_or(export.repository == from.identity, |c| {
                        c.node.ty == "service"
                            && c.export.repository == export.repository
                            && c.node.root == node.root
                            && (c.node.address == node.address
                                || export.iac_references.iter().any(|r| {
                                    r.root == node.root
                                        && r.source == c.node.address
                                        && r.target == node.address
                                        && r.via
                                            .as_deref()
                                            .is_some_and(|v| v.starts_with("networks."))
                                }))
                    })
                } else {
                    false
                }
            })
            .collect()
    }

    fn through_service(
        &self,
        service_export: &Export,
        service: &Node,
        consumer: &Consumer,
        prefix: Option<&str>,
        reason: &str,
        ambiguous: bool,
    ) -> Vec<Link> {
        let Some(wanted) = consumer.contract.as_deref() else {
            return Vec::new();
        };
        let selected = if service.ty == "service" {
            vec![service]
        } else {
            self.selected
                .get(&(
                    service_export.repository.as_str(),
                    service.root.as_str(),
                    service.address.as_str(),
                ))
                .cloned()
                .unwrap_or_default()
        };
        let mut candidates = Vec::new();
        for workload in selected {
            for execution in self.running(service_export, workload) {
                for &(repo, provider) in &self.providers {
                    if repo.identity != execution.repository
                        || !inside(&provider.site, &repo.identity, &execution.path)
                    {
                        continue;
                    }
                    let gateway = if fits(&provider.contract, wanted) {
                        None
                    } else if let Some(prefix) = prefix {
                        let Some((method, path)) = route(&provider.contract) else {
                            continue;
                        };
                        if !fits(&contract(method, &join(prefix, path)), wanted) {
                            continue;
                        }
                        None
                    } else {
                        let Some((_, why)) =
                            gateway_contracts(provider).find(|(g, _)| fits(g, wanted))
                        else {
                            continue;
                        };
                        Some(why)
                    };
                    let reason =
                        gateway.map_or_else(|| reason.into(), |why| format!("{reason}; {why}"));
                    let reason = if route(&provider.contract)
                        .is_some_and(|(method, _)| method == super::ANY)
                    {
                        format!("{reason}; any method")
                    } else {
                        reason
                    };
                    candidates.push(Link::new(
                        consumer.site.clone(),
                        provider.site.clone(),
                        wanted.into(),
                        if ambiguous {
                            Level::Ambiguous
                        } else {
                            Level::Inferred
                        },
                        reason,
                        None,
                        None,
                        None,
                    ));
                }
            }
        }
        // As in the ordinary gateway matcher, a code path wins within its repository.
        {
            let direct = candidates
                .iter()
                .filter_map(|l| {
                    self.providers
                        .iter()
                        .find(|(_, p)| p.site == l.provider && fits(&p.contract, wanted))
                        .map(|(r, _)| r.identity.as_str())
                })
                .collect::<BTreeSet<_>>();
            candidates.retain(|l| {
                self.providers.iter().any(|(r, p)| {
                    p.site == l.provider
                        && (!direct.contains(r.identity.as_str()) || fits(&p.contract, wanted))
                })
            });
        }
        candidates
    }

    /// The links a call to `host` takes through an Ingress or a Service, or `None` when no route
    /// reaches code: the host is then left to `[serve] hosts`, as before infrastructure was read.
    fn links(
        &self,
        consumer: &Consumer,
        from: &Repository,
        host: &str,
        caller: Option<&Location>,
        evidence: Option<&str>,
    ) -> Option<Vec<Link>> {
        let wanted = consumer.contract.as_deref()?;
        let (_, path) = route(wanted)?;
        let host = host.to_ascii_lowercase();
        let host_without_port = host
            .rsplit_once(':')
            .filter(|(_, p)| p.parse::<u16>().is_ok())
            .map_or(host.as_str(), |(h, _)| h);
        let mut ingresses = self
            .exports
            .iter()
            .flat_map(|e| {
                e.iac
                    .iter()
                    .flat_map(move |n| n.ingress.iter().map(move |r| (e, n, r)))
            })
            .filter(|(_, _, r)| {
                r.host == host_without_port
                    && (r.prefix.is_empty()
                        || path == r.prefix
                        || path
                            .strip_prefix(&r.prefix)
                            .is_some_and(|p| p.starts_with('/')))
            })
            .collect::<Vec<_>>();
        if !ingresses.is_empty() {
            if !self.ingress_ready {
                return None;
            }
            let longest = ingresses
                .iter()
                .map(|(_, _, r)| r.prefix.len())
                .max()
                .unwrap_or(0);
            ingresses.retain(|(_, _, r)| r.prefix.len() == longest);
            let ambiguous = ingresses.len() > 1;
            let found: Vec<_> = ingresses
                .iter()
                .flat_map(|(e, n, r)| {
                    let service = e
                        .iac
                        .iter()
                        .find(|s| s.root == n.root && s.address == r.service && s.ty == "Service");
                    service.into_iter().flat_map(move |s| {
                        let mut reason = format!(
                            "Ingress {} (root {}) prefix {} through Service {} (root {})",
                            n.identity(&e.repository),
                            n.root,
                            if r.prefix.is_empty() { "/" } else { &r.prefix },
                            s.identity(&e.repository),
                            s.root
                        );
                        if let Some(evidence) = evidence {
                            reason.push_str(&format!("; {evidence}"));
                        }
                        self.through_service(e, s, consumer, Some(&r.prefix), &reason, ambiguous)
                    })
                })
                .collect();
            return (!found.is_empty()).then_some(found);
        }
        let services = self.services(&host, caller, from);
        let ambiguous = services.len() > 1;
        let found: Vec<_> = services
            .iter()
            .flat_map(|(export, node)| {
                let kind = if node.ty == "service" {
                    "Compose service"
                } else {
                    "Service"
                };
                let mut reason = format!(
                    "{kind} {} (root {})",
                    node.identity(&export.repository),
                    node.root
                );
                if let Some(evidence) = evidence {
                    reason.push_str(&format!("; {evidence}"));
                }
                self.through_service(export, node, consumer, None, &reason, ambiguous)
            })
            .collect();
        (!found.is_empty()).then_some(found)
    }
}

fn base_url(value: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = value.split_once("://")?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let at = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..at];
    if host.is_empty() || host.contains('@') || host.chars().any(char::is_whitespace) {
        return None;
    }
    let path = rest[at..].split(['?', '#']).next().unwrap_or_default();
    Some((host, path))
}

fn collapse(mut links: Vec<Link>) -> Vec<Link> {
    let mut grouped = BTreeMap::<LinkKey, (Link, BTreeSet<String>)>::new();
    for link in links.drain(..) {
        let reason = link.reason.clone();
        let entry = grouped
            .entry(key(&link))
            .or_insert_with(|| (link.clone(), BTreeSet::new()));
        if link.level == Level::Ambiguous {
            entry.0.level = Level::Ambiguous;
        }
        entry.1.insert(reason);
    }
    let mut links = grouped
        .into_values()
        .map(|(mut link, reasons)| {
            link.reason = reasons.into_iter().collect::<Vec<_>>().join("; ");
            link
        })
        .collect::<Vec<_>>();
    let counts = links
        .iter()
        .fold(BTreeMap::<_, usize>::new(), |mut counts, l| {
            *counts.entry(l.contract.clone()).or_default() += 1;
            counts
        });
    for link in &mut links {
        if counts[&link.contract] > 1 {
            link.level = Level::Ambiguous;
        }
    }
    links
}

pub(super) fn prepare(exports: &mut [Export], repositories: &[Repository]) {
    if !exports.iter().any(|e| {
        e.consumers.iter().any(|c| {
            c.contract
                .as_deref()
                .is_some_and(|c| c.starts_with("http::"))
        })
    }) {
        return;
    }
    if !exports.iter().any(|e| {
        e.iac.iter().any(|n| {
            n.ty == "Service"
                || n.ty == "service"
                || !n.ingress.is_empty()
                || !n.environment.is_empty()
        })
    }) {
        return;
    }
    let environments = Environments::new(exports, repositories);
    let mut routing = Routing::new(exports, repositories);
    let aliases = routing.aliases();
    routing.ingress_ready = aliases.is_some();
    let mut aliases = aliases.unwrap_or_default();
    let mut prepared = Vec::new();
    let mut replacements = Vec::new();
    for (export, repo) in exports.iter().zip(repositories) {
        let mut out = Prepared::default();
        let mut consumers = Vec::new();
        for consumer in &export.consumers {
            let Some(wanted) = consumer
                .contract
                .as_deref()
                .filter(|c| c.starts_with("http::"))
            else {
                consumers.push(consumer.clone());
                continue;
            };
            let via = consumer.via.as_deref().unwrap_or_default();
            let (via, proxy) = via
                .split_once(PROXY_VIA)
                .map_or((via, None), |(v, p)| (v, Some(p)));
            if let Some(variable) = via.strip_prefix("env:") {
                let scoped =
                    environments.scoped_values(&export.repository, &consumer.site, variable);
                if !scoped.is_empty() {
                    let mut links = Vec::new();
                    let mut resolved = Vec::new();
                    for value in scoped {
                        let Some((host, base)) = value.value.and_then(base_url) else {
                            continue;
                        };
                        let Some((method, path)) = route(wanted) else {
                            continue;
                        };
                        let mut consumer = consumer.clone();
                        consumer.contract = Some(contract(method, &join(base, path)));
                        let caller = routing.location(value.workload, value.root);
                        let found = routing
                            .links(&consumer, repo, host, caller, Some(&value.reason))
                            .unwrap_or_else(|| {
                                let mut by_host = consumer.clone();
                                by_host.via = Some(format!(
                                    "host:{host}{}",
                                    proxy.map_or_else(String::new, |p| format!("{PROXY_VIA}{p}"))
                                ));
                                Http.link(&by_host, repo, &routing.providers, exports)
                                    .into_iter()
                                    .map(|mut l| {
                                        if l.level != Level::Ambiguous {
                                            l.level = Level::Inferred;
                                        }
                                        l.reason.push_str(&format!("; {}", value.reason));
                                        l
                                    })
                                    .collect()
                            });
                        links.extend(found);
                        resolved.push(consumer);
                    }
                    let links = collapse(links);
                    out.explained.extend(links.iter().map(key));
                    out.links.extend(links);
                    if resolved.is_empty() {
                        consumers.push(consumer.clone());
                    } else {
                        crate::index::sort_consumers(&mut resolved);
                        resolved.dedup_by(|a, b| {
                            a.site == b.site
                                && a.contract == b.contract
                                && a.via == b.via
                                && a.root == b.root
                        });
                        consumers.extend(resolved);
                    }
                    continue;
                }
            }
            consumers.push(consumer.clone());
            let mut inferred = false;
            let links = if let Some(host) = via.strip_prefix("host:") {
                let callers = routing.callers(&export.repository, &consumer.site);
                let callers = if callers.is_empty() {
                    vec![None]
                } else {
                    callers.into_iter().map(Some).collect()
                };
                let mut links = Vec::new();
                for caller in callers {
                    if let Some(found) = routing.links(consumer, repo, host, caller, None) {
                        inferred = true;
                        links.extend(found);
                    }
                }
                if inferred {
                    collapse(links)
                } else {
                    Http.link(consumer, repo, &routing.providers, exports)
                }
            } else {
                Http.link(consumer, repo, &routing.providers, exports)
            };
            if inferred {
                out.explained.extend(links.iter().map(key));
            }
            out.links.extend(links);
        }
        crate::index::sort_consumers(&mut consumers);
        prepared.push(out);
        replacements.push(consumers);
    }
    for ((export, prepared), consumers) in exports.iter_mut().zip(prepared).zip(replacements) {
        export.http = Some(prepared);
        export.consumers = consumers;
        for provider in &mut export.providers {
            if let Some(forms) = aliases.remove(&(provider.site.clone(), provider.contract.clone()))
            {
                for form in forms {
                    if !provider.gateways.contains(&form) {
                        provider.gateways.push(form);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::service_name;

    #[test]
    fn kubernetes_service_dns_forms_keep_the_optional_namespace_and_port() {
        for (host, namespace) in [
            ("orders", None),
            ("orders.shop", Some("shop")),
            ("orders.shop.svc", Some("shop")),
            ("orders.shop.svc.cluster.local", Some("shop")),
        ] {
            assert_eq!(service_name(host), Some(("orders", namespace)));
            let port = format!("{host}:8080");
            assert_eq!(service_name(&port), Some(("orders", namespace)));
        }
        for host in [
            "",
            "orders..shop",
            "-orders",
            "orders-",
            "orders.shop.svc.cluster",
            "orders.shop.svc.example.com",
            "orders:port",
            "orders:65536",
            "orders:80:90",
            "[::1]:80",
            "orders/shop",
        ] {
            assert_eq!(service_name(host), None, "{host}");
        }
    }
}
