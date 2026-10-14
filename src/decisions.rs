//! `meada.decisions.toml` (ADR 0006): a repository's resolutions of its ambiguous links and
//! unresolved consumers. Replaying it at query time gives those items a link at the level
//! `decided`; a decision that no longer fits is ignored. The file is an input like the source
//! tree: indexing and replay read no model and no network.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::config;
use crate::contract::{Level, Link};
use crate::index::Export;
use crate::workspace::{Missing, Workspace};

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

/// Why a decision does not apply. A decision that waits on a missing peer has none: it is
/// neither applied nor stale.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    /// No call with the decision's `call` id is in the repository.
    CallGone,
    /// The call is there, but the core resolves it on its own or it is no item of the
    /// decision's kind: nothing is left to decide.
    ItemNotOpen,
    /// No repository of the workspace serves the target's contract.
    TargetGone,
    /// The target is served, but is not among the candidates of the ambiguous link.
    NotCandidate,
}

/// A decision that does not apply.
#[derive(Serialize, Debug)]
pub struct Stale {
    pub call: String,
    pub target: String,
    pub cause: Cause,
}

/// A decision whose target repository is a peer the workspace names and cannot find.
#[derive(Debug)]
pub struct Waiting {
    pub call: String,
    pub target: String,
    /// The identity of the missing peer.
    pub peer: String,
}

/// What the decisions of one repository come to, once judged against the current index.
#[derive(Debug)]
pub struct Evaluation {
    pub repository: String,
    /// In file order.
    pub stale: Vec<Stale>,
    /// In file order.
    pub waiting: Vec<Waiting>,
}

enum Verdict {
    /// Links the call to this provider.
    Applies(String),
    Stale(Cause),
    Waiting(String),
}

/// Judges `d`, a decision of the repository whose index is `export`, against `core`, the
/// workspace's links before any decision. The checks run in this order, the first that fails
/// decides: the call is there, its item is still open (an ambiguous link, or an unresolved
/// consumer), the target's repository is not a missing peer, the target is still served, and,
/// for an ambiguous link, it is still among the link's candidates.
fn judge(
    d: &Decision,
    export: &Export,
    exports: &[Export],
    core: &[Link],
    missing: &[Missing],
) -> Verdict {
    let at = |l: &&Link| l.consumer == d.call && l.level == Level::Ambiguous;
    if !(export.calls.iter().any(|c| c.id == d.call)
        || export.consumers.iter().any(|c| c.site == d.call))
    {
        return Verdict::Stale(Cause::CallGone);
    }
    let open = match d.kind {
        Kind::AmbiguousLink => core.iter().any(|l| at(&l)),
        Kind::DynamicUrl => {
            (export.consumers.iter()).any(|c| c.site == d.call && c.contract.is_none())
        }
    };
    if !open {
        return Verdict::Stale(Cause::ItemNotOpen);
    }
    let Some((identity, contract)) = d.target.split_once("::") else {
        return Verdict::Stale(Cause::TargetGone);
    };
    if let Some(peer) = missing.iter().find(|m| m.repo == identity) {
        return Verdict::Waiting(peer.repo.clone());
    }
    // ponytail: a repository serving one contract from several sites links to the first by
    // site; the target names no site.
    let provider = exports
        .iter()
        .filter(|e| e.repository == identity)
        .flat_map(|e| &e.providers)
        .filter(|p| p.contract == contract)
        .map(|p| &p.site)
        .min();
    let Some(provider) = provider else {
        return Verdict::Stale(Cause::TargetGone);
    };
    match d.kind {
        Kind::DynamicUrl => Verdict::Applies(provider.clone()),
        Kind::AmbiguousLink => {
            let prefix = format!("{identity}::");
            core.iter()
                .filter(at)
                .find(|l| l.contract == contract && l.provider.starts_with(&prefix))
                .map_or(Verdict::Stale(Cause::NotCandidate), |l| {
                    Verdict::Applies(l.provider.clone())
                })
        }
    }
}

/// Judges every repository's decisions against `links`, the workspace's links before
/// decisions, and applies those that fit: each replaces the ambiguous link of its call, or
/// adds the missing one, with a single link at the level `decided`. A call decided twice
/// keeps its first decision. `exports` follow the workspace's repositories. Returns the links,
/// sorted, and for each repository the decisions that did not apply. A stale decision or one
/// that waits on a missing peer changes no link: the core's stands.
pub(crate) fn replay(
    mut links: Vec<Link>,
    workspace: &Workspace,
    exports: &[Export],
) -> (Vec<Link>, Vec<Evaluation>) {
    let mut decided = HashSet::new();
    let mut evaluations = Vec::new();
    let mut applied = Vec::new();
    for (repo, export) in workspace.repos.iter().zip(exports) {
        let mut evaluation = Evaluation {
            repository: repo.identity.clone(),
            stale: Vec::new(),
            waiting: Vec::new(),
        };
        for d in &repo.decisions.items {
            let verdict = match judge(d, export, exports, &links, &workspace.missing) {
                Verdict::Applies(_) if !decided.insert(&d.call) => {
                    Verdict::Stale(Cause::ItemNotOpen)
                }
                other => other,
            };
            let (call, target) = (d.call.clone(), d.target.clone());
            match verdict {
                Verdict::Applies(provider) => applied.push((d, provider)),
                Verdict::Stale(cause) => evaluation.stale.push(Stale {
                    call,
                    target,
                    cause,
                }),
                Verdict::Waiting(peer) => evaluation.waiting.push(Waiting { call, target, peer }),
            }
        }
        evaluations.push(evaluation);
    }
    for (d, provider) in applied {
        let (_, contract) = d.target.split_once("::").expect("a judged target has `::`");
        links.retain(|l| !(l.consumer == d.call && l.level == Level::Ambiguous));
        links.push(d.link(&provider, contract));
    }
    links.sort_by(|a, b| (&a.consumer, &a.provider).cmp(&(&b.consumer, &b.provider)));
    (links, evaluations)
}
