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
use crate::http;
use crate::index::Export;
use crate::items::{Candidate, ItemKind, OpenItem};
use crate::msg::Msg;
use crate::workspace::{Missing, Workspace};

const FILE: &str = "meada.decisions.toml";

#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Decisions {
    #[serde(default, rename = "decision")]
    pub items: Vec<Decision>,
}

#[derive(Deserialize, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A call with several fitting providers.
    AmbiguousLink,
    /// A consumer whose URL Meada could not evaluate.
    DynamicUrl,
}

impl Kind {
    /// The kind of decision that resolves `item`.
    pub(crate) fn of(item: &OpenItem) -> Self {
        match item.kind {
            ItemKind::Ambiguous => Kind::AmbiguousLink,
            ItemKind::Unresolved => Kind::DynamicUrl,
        }
    }
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
            (true, Some(_)) => return Err(Msg::DecisionPHuman.text()),
            (false, None) => return Err(Msg::DecisionPMissing.text()),
            (false, Some(p)) if !probability(p) => return Err(Msg::DecisionPRange.text()),
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

/// Whether `p` is a probability: a number in [0,1], so neither NaN nor infinite. The one check
/// behind the file's `p`, a decider's answer and `--min-p`.
pub(crate) fn probability(p: f64) -> bool {
    (0.0..=1.0).contains(&p)
}

/// `<root>/meada.decisions.toml`, or no decisions when the file does not exist.
/// The file holds at most one entry per `call`: a repeat is a parse error naming its line.
pub fn load(root: &Path) -> Result<Decisions> {
    let decisions: Decisions = config::read(root, FILE)?;
    let mut seen = HashSet::new();
    let Some(repeat) = decisions.items.iter().find(|d| !seen.insert(&d.call)) else {
        return Ok(decisions);
    };
    let file = root.join(FILE);
    // The repeat is the second `call = "<call>"` line.
    let line = std::fs::read_to_string(&file)?
        .lines()
        .enumerate()
        .filter(|(_, l)| l.starts_with("call") && l.contains(&format!("\"{}\"", repeat.call)))
        .nth(1)
        .map_or(1, |(n, _)| n + 1);
    Err(format!(
        "{}:{line}: {}",
        file.display(),
        Msg::CallRepeated(&repeat.call).text()
    )
    .into())
}

/// The canonical text of `<root>/meada.decisions.toml` once `new` is in it: `new` replaces the
/// entry of the same call, stale or not, and the entries are sorted by `call`. No comments and
/// no timestamps, so the same decisions always give the same bytes.
pub(crate) fn canonical(root: &Path, new: Vec<Decision>) -> Result<String> {
    let mut all = load(root)?.items;
    all.retain(|d| new.iter().all(|n| n.call != d.call));
    all.extend(new);
    render(all)
}

/// The canonical text of the decisions file without its `stale` entries, with how many that
/// removed, or `None` when none of them is in the file. Waiting entries stay.
pub(crate) fn pruned(root: &Path, stale: &[Stale]) -> Result<Option<(String, usize)>> {
    let mut all = load(root)?.items;
    let before = all.len();
    for s in stale {
        all.retain(|d| d.call != s.call);
    }
    let removed = before - all.len();
    Ok(if removed == 0 {
        None
    } else {
        Some((render(all)?, removed))
    })
}

fn render(mut all: Vec<Decision>) -> Result<String> {
    all.sort_by(|a, b| a.call.cmp(&b.call));
    Ok(toml::to_string(&Decisions { items: all })?)
}

/// Writes `text`, from `canonical`, as `<root>/meada.decisions.toml`. It goes to a temporary
/// file first and is renamed over the old one, so a failed write keeps the old decisions.
pub(crate) fn save(root: &Path, text: &str) -> Result<()> {
    let temp = root.join(format!("{FILE}.tmp"));
    std::fs::write(&temp, text)?;
    Ok(std::fs::rename(temp, root.join(FILE))?)
}

impl Decision {
    /// A person's pick of `candidate` for `item`.
    pub(crate) fn human(item: &OpenItem, candidate: &Candidate) -> Self {
        Self {
            kind: Kind::of(item),
            call: item.id.clone(),
            target: Self::target(candidate),
            by: "human".into(),
            p: None,
        }
    }

    /// The `target` that picks `candidate`: its repository identity and contract.
    pub(crate) fn target(candidate: &Candidate) -> String {
        format!("{}::{}", candidate.repository, candidate.contract)
    }

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
            drift: None,
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

/// What the decisions of one repository come to, once evaluated against the current index.
#[derive(Debug)]
pub struct Evaluation {
    pub repository: String,
    /// In file order.
    pub stale: Vec<Stale>,
    /// In file order.
    pub waiting: Vec<Waiting>,
}

impl Evaluation {
    /// One line per stale decision, then one per waiting decision, as `status` and
    /// `meada decide --list` print them.
    pub fn lines(&self) -> impl Iterator<Item = String> {
        let stale = self.stale.iter().map(|s| {
            let (call, target, cause) = (&s.call, &s.target, s.cause);
            Msg::StaleDecision {
                call,
                target,
                cause,
            }
            .text()
        });
        let waiting = self.waiting.iter().map(|w| {
            let (call, target, peer) = (&w.call, &w.target, &w.peer);
            Msg::WaitingDecision { call, target, peer }.text()
        });
        stale.chain(waiting)
    }
}

/// What a decision comes to against the current index.
enum Fate {
    /// Links the call to this provider through this contract.
    Applies(String, String),
    Stale(Cause),
    Waiting(String),
}

/// Evaluates `d`, a decision of the repository whose index is `export`, against `core`, the
/// workspace's links before any decision. The checks run in this order, the first that fails
/// decides: the call is there, the target's repository is not a missing peer (the decision
/// waits, whatever else), its item is still open (an ambiguous link, or an unresolved
/// consumer), the target is still served, and, for an ambiguous link, it is still among the
/// link's candidates.
fn evaluate(
    d: &Decision,
    export: &Export,
    exports: &[Export],
    core: &[Link],
    missing: &[Missing],
) -> Fate {
    let at = |l: &&Link| l.consumer == d.call && l.level == Level::Ambiguous;
    if !(export.calls.iter().any(|c| c.id == d.call)
        || export.consumers.iter().any(|c| c.site == d.call))
    {
        return Fate::Stale(Cause::CallGone);
    }
    let target = d.target.split_once("::");
    // Without the peer the core cannot see its providers, so whether the item is open, or the
    // target served, is unknown: the decision waits.
    if let Some(peer) = target.and_then(|(identity, _)| missing.iter().find(|m| m.repo == identity))
    {
        return Fate::Waiting(peer.repo.clone());
    }
    let open = match d.kind {
        Kind::AmbiguousLink => core.iter().any(|l| at(&l)),
        Kind::DynamicUrl => {
            (export.consumers.iter()).any(|c| c.site == d.call && c.contract.is_none())
        }
    };
    if !open {
        return Fate::Stale(Cause::ItemNotOpen);
    }
    let Some((identity, contract)) = target else {
        return Fate::Stale(Cause::TargetGone);
    };
    // ponytail: a repository serving one contract from several sites links to the first by
    // site; the target names no site.
    let served: Vec<_> = exports
        .iter()
        .filter(|e| e.repository == identity)
        .flat_map(|e| &e.providers)
        .filter(|p| p.contract == contract)
        .collect();
    let Some(provider) = served.iter().map(|p| &p.site).min() else {
        return Fate::Stale(Cause::TargetGone);
    };
    match d.kind {
        Kind::DynamicUrl => Fate::Applies(provider.clone(), contract.into()),
        // The ambiguous link to a provider of the target, by its code path or its gateway
        // path; the decided link keeps the contract the call uses.
        Kind::AmbiguousLink => core
            .iter()
            .filter(at)
            .find(|l| (served.iter()).any(|p| p.site == l.provider && http::serves(p, &l.contract)))
            .map_or(Fate::Stale(Cause::NotCandidate), |l| {
                Fate::Applies(l.provider.clone(), l.contract.clone())
            }),
    }
}

/// Evaluates every repository's decisions against `links`, the workspace's links before
/// decisions, and applies those that fit: each replaces the ambiguous link of its call, or
/// adds the missing one, with a single link at the level `decided`. `exports` follow the workspace's repositories. Returns the links,
/// sorted, and for each repository the decisions that did not apply. A stale decision or one
/// that waits on a missing peer changes no link: the core's stands.
pub(crate) fn replay(
    mut links: Vec<Link>,
    workspace: &Workspace,
    exports: &[Export],
) -> (Vec<Link>, Vec<Evaluation>) {
    let mut evaluations = Vec::new();
    let mut applied = Vec::new();
    for (repo, export) in workspace.repos.iter().zip(exports) {
        let mut evaluation = Evaluation {
            repository: repo.identity.clone(),
            stale: Vec::new(),
            waiting: Vec::new(),
        };
        for d in &repo.decisions.items {
            let fate = evaluate(d, export, exports, &links, &workspace.missing);
            let (call, target) = (d.call.clone(), d.target.clone());
            match fate {
                Fate::Applies(provider, contract) => applied.push((d, provider, contract)),
                Fate::Stale(cause) => evaluation.stale.push(Stale {
                    call,
                    target,
                    cause,
                }),
                Fate::Waiting(peer) => evaluation.waiting.push(Waiting { call, target, peer }),
            }
        }
        evaluations.push(evaluation);
    }
    for (d, provider, contract) in applied {
        links.retain(|l| !(l.consumer == d.call && l.level == Level::Ambiguous));
        links.push(d.link(&provider, &contract));
    }
    links.sort_by(|a, b| (&a.consumer, &a.provider).cmp(&(&b.consumer, &b.provider)));
    (links, evaluations)
}
