//! `meada.decisions.toml` (ADR 0006): a repository's resolutions of its ambiguous links and
//! unresolved consumers. Replaying it at query time gives those items a link at the level
//! `decided`; a decision that no longer fits is ignored. The file is an input like the source
//! tree: indexing and replay read no model and no network.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::config;
use crate::contract::{self, Level, Link};
use crate::index::{Consumer, Export};
use crate::items::{self, Candidate, OpenItem};
use crate::msg::Msg;
use crate::workspace::{Missing, Workspace};

const FILE: &str = "meada.decisions.toml";

#[derive(Deserialize, Serialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct Decisions {
    #[serde(default, rename = "decision")]
    pub items: Vec<Decision>,
}

#[derive(Deserialize, Serialize, Clone)]
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

pub use meada_core::export::DecisionKind as Kind;

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
    load_up_to(root, None)
}

/// `load`, reading at most `max` bytes of the file, through no symbolic link, when it is set
/// (`config::read_own_up_to`, a mirror's file).
pub(crate) fn load_up_to(root: &Path, max: Option<u64>) -> Result<Decisions> {
    let decisions: Decisions = config::read_own_up_to(root, FILE, max)?.unwrap_or_default();
    let mut seen = HashSet::new();
    let Some(repeat) = decisions.items.iter().find(|d| !seen.insert(&d.call)) else {
        return Ok(decisions);
    };
    let file = root.join(FILE);
    // The repeat is the second `call = "<call>"` line.
    let line = (config::text(&file, max)?.unwrap_or_default())
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
        if matches!(
            s.cause,
            Cause::WrongKind | Cause::UnknownRepository | Cause::NoCandidates
        ) {
            continue;
        }
        all.retain(|d| d.call != s.call || d.target != s.target);
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

/// Refuses a `<root>/meada.decisions.toml` that is a symbolic link, a dangling one included
/// (`config::unlinked`), before a run that can `save` asks or sends anything: the rename would
/// replace the link with a regular file and leave its target as it was. Meada writes the file
/// in the repository root only (ADR 0006).
pub(crate) fn writable(root: &Path) -> Result<()> {
    let file = root.join(FILE);
    match config::unlinked(&file) {
        Ok(None) => Err(format!("{}: {}", file.display(), Msg::DecisionsLinked.text()).into()),
        // A missing file is created by `save`; another error is the load's or the save's.
        _ => Ok(()),
    }
}

/// A newly created staging file in the same directory as the decisions. Only this process
/// can have created the name. Drop removes a staging file after a failed write or rename.
struct Staged(PathBuf);

impl Staged {
    fn new(root: &Path) -> Result<(Self, File)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("{FILE}.tmp.{}.{sequence}", std::process::id()));
            match File::create_new(&path) {
                Ok(file) => return Ok((Self(path), file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(at_path(&path, error)),
            }
        }
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn at_path(path: &Path, error: io::Error) -> Box<dyn std::error::Error> {
    format!("{}: {error}", path.display()).into()
}

/// Check that the directory accepts a same-directory staging file before contacting the model.
pub(crate) fn check_destination(root: &Path) -> Result<()> {
    writable(root)?;
    let (staged, file) = Staged::new(root)?;
    drop(file);
    drop(staged);
    Ok(())
}

/// Serializes read/merge/write operations by saving commands, including simultaneous CLI runs.
pub(crate) fn locked<T>(root: &Path, update: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = root.join(".meada/decisions.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| at_path(&path, error))?;
    lock.lock().map_err(|error| at_path(&path, error))?;
    writable(root)?;
    update()
}

/// Merge picks against the latest file before the atomic save.
pub(crate) fn record(root: &Path, new: Vec<Decision>) -> Result<()> {
    locked(root, || save(root, &canonical(root, new)?))
}

/// Write a complete canonical file beside its destination before replacing the previous one.
pub(crate) fn save(root: &Path, text: &str) -> Result<()> {
    let destination = root.join(FILE);
    let (staged, mut file) = Staged::new(root)?;
    file.write_all(text.as_bytes())
        .map_err(|error| at_path(&staged.0, error))?;
    file.sync_all().map_err(|error| at_path(&staged.0, error))?;
    drop(file);
    writable(root)?;
    fs::rename(&staged.0, &destination).map_err(|error| at_path(&destination, error))
}

impl Decision {
    /// A person's pick of `candidate` for `item`.
    pub(crate) fn human(item: &OpenItem, candidate: &Candidate) -> Self {
        Self {
            kind: item.decision_kind(),
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
    pub(crate) fn reason(&self) -> String {
        match self.p {
            Some(p) => format!("decided by {} (p={p:.2})", self.by),
            None => format!("decided by {}", self.by),
        }
    }

    fn link(&self, provider: &str, contract: &str) -> Link {
        Link::new(
            self.call.clone(),
            provider.into(),
            contract.into(),
            Level::Decided,
            self.reason(),
            Some(self.by.clone()),
            self.p,
            None,
        )
    }
}

pub use meada_core::export::{Cause, Stale};

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
                expected_kind: s.expected_kind,
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
    WrongKind(Kind),
    Waiting(String),
}

/// Evaluates `d`, a decision of the repository whose index is `export`, against `core`, the
/// workspace's links before any decision. The checks run in this order, the first that fails
/// decides: the call is there, the target's repository is not a missing peer (the decision
/// waits, whatever else), its item is still open (an ambiguous link, or an unresolved
/// consumer), the target is still served, and it is still among the item's candidates: the
/// link's, or those `meada decide` lists for the unresolved consumer (#806). The workspace
/// causes are read only if an unresolved item has a provider that passes the candidate rule.
fn evaluate(
    d: &Decision,
    export: &Export,
    exports: &[Export],
    core: &[Link],
    missing: &[Missing],
    owned: &dyn Fn(&Consumer) -> bool,
) -> Fate {
    let at = |l: &&Link| l.item_site() == d.call && contract::consumer_item(l, exports);
    if !(export.calls.iter().any(|c| c.id == d.call)
        || export.consumers.iter().any(|c| c.item_site() == d.call))
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
    let consumer =
        (export.consumers.iter()).find(|c| c.item_site() == d.call && c.contract.is_none());
    let own_kind = match (core.iter().any(|l| at(&l)), consumer.is_some()) {
        // A call can have multiple sides. Keep the kind the decision addresses if both
        // exist, as replay did before it reported a kind mismatch.
        (true, true) => d.kind,
        (true, false) => Kind::AmbiguousLink,
        (false, true) => Kind::DynamicUrl,
        (false, false) => return Fate::Stale(Cause::ItemNotOpen),
    };
    if own_kind != d.kind {
        return Fate::WrongKind(own_kind);
    }
    let Some((identity, contract)) = target else {
        return Fate::Stale(Cause::UnknownRepository);
    };
    let Some(repository) = exports.iter().find(|e| e.repository == identity) else {
        return Fate::Stale(Cause::UnknownRepository);
    };
    // ponytail: a repository serving one contract from several sites links to the first by
    // site; the target names no site.
    let served: Vec<_> = repository
        .providers
        .iter()
        .filter(|p| p.contract == contract)
        .collect();
    if served.is_empty() {
        return Fate::Stale(Cause::TargetGone);
    }
    match d.kind {
        // A provider of the target that `meada decide` lists for the consumer: none for a
        // consumer a kind owns.
        Kind::DynamicUrl => {
            let consumer = consumer.expect("a dynamic URL has an unresolved consumer");
            let fits = |provider| items::fits(consumer, provider);
            if let Some(site) = served
                .iter()
                .copied()
                .filter(|p| fits(p))
                .map(|p| &p.site)
                .min()
            {
                return if owned(consumer) {
                    Fate::Stale(Cause::NoCandidates)
                } else {
                    Fate::Applies(site.clone(), contract.into())
                };
            }
            let any_fit = exports.iter().flat_map(|e| &e.providers).any(fits);
            if any_fit && !owned(consumer) {
                Fate::Stale(Cause::NotCandidate)
            } else {
                Fate::Stale(Cause::NoCandidates)
            }
        }
        // The ambiguous link to a provider of the target, by its code path or its gateway
        // path; the decided link keeps the contract the call uses.
        Kind::AmbiguousLink => core
            .iter()
            .filter(at)
            .find(|l| (served.iter()).any(|p| contract::link_candidate(p, l, exports)))
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
    links: Vec<Link>,
    workspace: &Workspace,
    exports: &[Export],
    include: impl Fn(&Decision) -> bool,
) -> (Vec<Link>, Vec<Evaluation>) {
    replay_repositories(
        links,
        &workspace.repos,
        &workspace.missing,
        exports,
        include,
    )
}

/// The same evaluation for a kind's execution projection. Raw links remain the input to
/// open-item validation; derived environment and routing facts consume only this result.
pub(crate) fn replay_repositories(
    mut links: Vec<Link>,
    repositories: &[crate::workspace::Repository],
    missing: &[Missing],
    exports: &[Export],
    include: impl Fn(&Decision) -> bool,
) -> (Vec<Link>, Vec<Evaluation>) {
    let mut evaluations = Vec::new();
    let mut applied = Vec::new();
    let causes = OnceCell::new();
    let owned = |consumer: &Consumer| {
        causes
            .get_or_init(|| items::causes(exports))
            .contains_key(&consumer.site)
    };
    for (repo, export) in repositories.iter().zip(exports) {
        let mut evaluation = Evaluation {
            repository: repo.identity.clone(),
            stale: Vec::new(),
            waiting: Vec::new(),
        };
        for d in repo.decisions.items.iter().filter(|d| include(d)) {
            let fate = evaluate(d, export, exports, &links, missing, &owned);
            let (call, target) = (d.call.clone(), d.target.clone());
            match fate {
                Fate::Applies(provider, contract) => applied.push((d, provider, contract)),
                Fate::Stale(cause) => evaluation.stale.push(Stale::new(call, target, cause)),
                Fate::WrongKind(kind) => {
                    let mut stale = Stale::new(call, target, Cause::WrongKind);
                    stale.expected_kind = Some(kind);
                    evaluation.stale.push(stale);
                }
                Fate::Waiting(peer) => evaluation.waiting.push(Waiting { call, target, peer }),
            }
        }
        evaluations.push(evaluation);
    }
    for (d, provider, contract) in applied {
        let source = links
            .iter()
            .find(|l| l.item_site() == d.call)
            .map(|l| (l.consumer.clone(), l.root.clone()));
        links.retain(|l| !(l.item_site() == d.call && l.level == Level::Ambiguous));
        let mut decided = d.link(&provider, &contract);
        if let Some((consumer, root)) = source {
            decided.consumer = consumer;
            decided.root = root;
        }
        links.push(decided);
    }
    links.sort_by(|a, b| (&a.consumer, &a.provider).cmp(&(&b.consumer, &b.provider)));
    (links, evaluations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Gateway, Provider};
    use std::cell::Cell;

    #[test]
    fn a_decision_uses_the_first_fitting_site_and_only_reads_causes_when_needed() {
        let call = "example.com/shop::web.ts::load::fetch#0";
        let contract = "http::GET::/orders/{}";
        let shop = Export {
            repository: "example.com/shop".into(),
            consumers: vec![Consumer {
                site: call.into(),
                contract: None,
                via: None,
                shape: None,
                method: Some("GET".into()),
                identity: None,
                fragments: vec!["/shop/orders/".into()],
                lead: None,
                hops: Vec::new(),
                root: None,
            }],
            ..Export::default()
        };
        let api = Export {
            repository: "example.com/api".into(),
            providers: vec![
                Provider {
                    contract: contract.into(),
                    site: "example.com/api::a/route.ts::GET".into(),
                    ..Provider::default()
                },
                Provider {
                    contract: contract.into(),
                    site: "example.com/api::b/route.ts::GET".into(),
                    gateways: vec![Gateway {
                        path: "/shop/orders/{}".into(),
                        reason: "gateway prefix /shop".into(),
                    }],
                    ..Provider::default()
                },
            ],
            ..Export::default()
        };
        let exports = [shop, api];
        let mut picked = Decision {
            kind: Kind::DynamicUrl,
            call: call.into(),
            target: "example.com/api::http::GET::/missing".into(),
            by: "human".into(),
            p: None,
        };
        let reads = Cell::new(0);
        let owned = |_: &Consumer| {
            reads.set(reads.get() + 1);
            false
        };
        assert!(matches!(
            evaluate(&picked, &exports[0], &exports, &[], &[], &owned),
            Fate::Stale(Cause::TargetGone)
        ));
        assert_eq!(reads.get(), 0);
        picked.target = format!("example.com/api::{contract}");
        let result = evaluate(&picked, &exports[0], &exports, &[], &[], &owned);
        assert!(
            matches!(result, Fate::Applies(site, _) if site == "example.com/api::b/route.ts::GET")
        );
        assert_eq!(reads.get(), 1);
    }
}
