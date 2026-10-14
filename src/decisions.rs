//! `meada.decisions.toml` (ADR 0006): a repository's resolutions of its ambiguous links and
//! unresolved consumers. Replaying it at query time gives those items a link at the level
//! `decided`; a decision that no longer fits is ignored. The file is an input like the source
//! tree: indexing and replay read no model and no network.

use std::path::Path;

use serde::Deserialize;

use crate::Result;
use crate::config;
use crate::contract::{Level, Link};
use crate::index::Export;
use crate::workspace::Repository;

const FILE: &str = "meada.decisions.toml";

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Decisions {
    #[serde(default, rename = "decision")]
    pub items: Vec<Decision>,
}

#[derive(Deserialize)]
#[serde(try_from = "Raw")]
pub struct Decision {
    pub kind: Kind,
    /// The call site of the item, as `meada export` names it.
    pub call: String,
    /// `<provider repository identity>::<contract identifier>`, such as
    /// `github.com/acme/api::http::GET::/users/{}`.
    pub target: String,
    /// `human` or a model id.
    pub by: String,
    /// The probability a model stated; never set for `human`.
    pub p: Option<f64>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A call with several fitting providers.
    AmbiguousLink,
    /// A consumer whose URL Meada could not evaluate.
    DynamicUrl,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    kind: Kind,
    call: String,
    target: String,
    by: String,
    p: Option<f64>,
}

impl TryFrom<Raw> for Decision {
    type Error = String;

    fn try_from(raw: Raw) -> std::result::Result<Self, String> {
        match (raw.by == "human", raw.p) {
            (true, Some(_)) => return Err("`p` is for a model, not for `human`".into()),
            (false, None) => return Err("`p` is missing for a model".into()),
            (false, Some(p)) if !(0.0..=1.0).contains(&p) => {
                return Err("`p` must be between 0 and 1".into());
            }
            _ => {}
        }
        Ok(Self {
            kind: raw.kind,
            call: raw.call,
            target: raw.target,
            by: raw.by,
            p: raw.p,
        })
    }
}

/// `<root>/meada.decisions.toml`, or no decisions when the file does not exist.
pub fn load(root: &Path) -> Result<Decisions> {
    config::read(root, FILE)
}

impl Decision {
    /// `decided by <by> (p=<p>)`, or `decided by human`.
    fn reason(&self) -> String {
        match self.p {
            Some(p) => format!("decided by {} (p={p:.2})", self.by),
            None => format!("decided by {}", self.by),
        }
    }

    fn link(&self, provider: &str, contract: &str) -> Link {
        Link {
            consumer: self.call.clone(),
            provider: provider.into(),
            contract: contract.into(),
            level: Level::Decided,
            reason: self.reason(),
            by: Some(self.by.clone()),
            p: self.p,
        }
    }
}

/// Applies every repository's decisions to `links`, the workspace's links before decisions.
/// `exports` follow `repos`. A decision applies when its item is still open (an ambiguous link
/// among whose candidates the target is, or an unresolved consumer) and its target is still
/// provided; it then replaces the ambiguous link, or adds the missing one, with a single link
/// at the level `decided`. Anything else is stale and ignored.
pub(crate) fn replay(links: &mut Vec<Link>, repos: &[Repository], exports: &[Export]) {
    for (repo, export) in repos.iter().zip(exports) {
        for d in &repo.decisions.items {
            let Some((identity, contract)) = d.target.split_once("::") else {
                continue;
            };
            let consumes = |contract: Option<&str>| {
                export
                    .consumers
                    .iter()
                    .any(|c| c.site == d.call && c.contract.as_deref() == contract)
            };
            match d.kind {
                Kind::AmbiguousLink => {
                    let open = |l: &Link| {
                        l.consumer == d.call
                            && l.level == Level::Ambiguous
                            && l.contract == contract
                    };
                    let prefix = format!("{identity}::");
                    let Some(provider) = links
                        .iter()
                        .find(|l| open(l) && l.provider.starts_with(&prefix))
                        .map(|l| l.provider.clone())
                    else {
                        continue;
                    };
                    if !consumes(Some(contract)) {
                        continue;
                    }
                    links.retain(|l| !open(l));
                    links.push(d.link(&provider, contract));
                }
                Kind::DynamicUrl => {
                    // ponytail: a repository serving one contract from several sites links to
                    // the first by site; the target names no site.
                    let provider = exports
                        .iter()
                        .filter(|e| e.repository == identity)
                        .flat_map(|e| &e.providers)
                        .filter(|p| p.contract == contract)
                        .map(|p| &p.site)
                        .min();
                    let linked = links
                        .iter()
                        .any(|l| l.consumer == d.call && l.level == Level::Decided);
                    if let (Some(provider), true, false) = (provider, consumes(None), linked) {
                        links.push(d.link(provider, contract));
                    }
                }
            }
        }
    }
    links.sort_by(|a, b| (&a.consumer, &a.provider).cmp(&(&b.consumer, &b.provider)));
}
