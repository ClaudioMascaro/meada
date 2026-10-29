//! What a subject may change in a workspace version (ADR 0015): `authorize_entries`, run by the
//! server under the refresh lock and by the browser before a save, and `compose_entries`, the
//! version a team operator's save stores. Entries are a version document's `repositories`
//! (`repo`, `path`, `owner` and `added_by` when there are, and the blocks), already validated.
//! Under the GitHub grant provider (ADR 0016, #333) both take the subject's login and the GitHub
//! grant provider's host; without it both are none and the functions answer as ADR 0015's. Pure:
//! no clock, network or file.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// A console subject's role. The routes never pass a `viewer`, which S2's role check refuses
/// first; one passed in is answered by the team operator's rules.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Operator,
    TeamOperator,
    Viewer,
}

/// What a subject reads: `*`, or a set of identities (a set holding `*` reads everything).
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(try_from = "GrantText")]
pub enum Grant {
    All,
    Only(BTreeSet<String>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum GrantText {
    All(String),
    Only(BTreeSet<String>),
}

impl TryFrom<GrantText> for Grant {
    type Error = String;

    fn try_from(text: GrantText) -> Result<Self, String> {
        match text {
            GrantText::All(all) if all == "*" => Ok(Grant::All),
            GrantText::All(other) => Err(format!("`{other}` is not a grant: `*` or identities")),
            GrantText::Only(only) => Ok(Grant::Only(only)),
        }
    }
}

impl Grant {
    pub fn reads(&self, identity: &str) -> bool {
        match self {
            Grant::All => true,
            Grant::Only(identities) => identities.contains(identity) || identities.contains("*"),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Add,
    Edit,
    Remove,
}

/// Why an entry's change is refused, in the order the rules are checked.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The identity is outside the grant; under the GitHub grant provider, an addition on its
    /// host is not refused for it.
    OutsideGrant,
    /// Under the GitHub grant provider, an addition on its host without `added_by`, or by a
    /// subject without a login.
    Unattributed,
    /// The current entry has no owner (an edit or a removal), or an addition has none.
    Ownerless,
    /// The owner differs between the current and the new entry, cleared included.
    OwnerChange,
    /// Under the GitHub grant provider, `added_by` differs between the current and the new entry.
    AddedByChange,
    /// The owner is not one of the subject's teams.
    NotYourTeam,
}

/// One entry the subject may not change. An `outside_grant` refusal names an identity the subject
/// may not read: the console counts those instead of naming them (ADR 0015, #324).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub identity: String,
    pub change: Change,
    pub reason: Reason,
}

/// An entry's identity, its `repo`.
pub fn repo(entry: &Json) -> &str {
    entry["repo"].as_str().unwrap_or_default()
}

fn owner(entry: &Json) -> Option<&str> {
    entry["owner"].as_str()
}

fn added_by(entry: &Json) -> Option<&str> {
    entry["added_by"].as_str()
}

/// Whether `identity` is on `host`, the GitHub grant provider's: its first segment.
pub fn on_host(identity: &str, host: &str) -> bool {
    identity.split_once('/').is_some_and(|(h, _)| h == host)
}

/// The subject's teams: its groups that own an entry of `current` whose identity `grant` reads.
pub fn teams<'a>(groups: &'a [String], grant: &Grant, current: &[Json]) -> Vec<&'a str> {
    let owned: BTreeSet<&str> = (current.iter())
        .filter(|entry| grant.reads(repo(entry)))
        .filter_map(owner)
        .collect();
    (groups.iter().map(String::as_str))
        .filter(|group| owned.contains(group))
        .collect()
}

/// The role a subject holds in the version whose entries are `current`, as admission computes
/// it (#324): the global operator stays one; anyone else is a team operator while one of its
/// `groups` owns an entry `grant` reads, else a viewer.
pub fn role(role: Role, groups: &[String], grant: &Grant, current: &[Json]) -> Role {
    match role {
        Role::Operator => Role::Operator,
        _ if teams(groups, grant, current).is_empty() => Role::Viewer,
        _ => Role::TeamOperator,
    }
}

/// The entries of the change from `current` to `new` that the subject may not make, each with
/// the first rule it fails; empty when the change is authorized. Entries pair by `repo`; one is
/// changed when anything but its position differs. Added and edited entries follow `new`, then
/// removed ones follow `current`. The global operator is refused nothing. Under the GitHub grant
/// provider (`github_host`, with the subject's `login`), an addition on its host is not refused
/// as outside the grant but needs its `added_by` (which `compose_entries` sets from `login`) and
/// a login, and an edit may not change `added_by`. An addition is new to `current`: the server's
/// save refuses one that collides with an identity it holds outside the grant itself.
pub fn authorize_entries(
    role: Role,
    groups: &[String],
    grant: &Grant,
    login: Option<&str>,
    github_host: Option<&str>,
    current: &[Json],
    new: &[Json],
) -> Vec<Refusal> {
    if role == Role::Operator {
        return Vec::new();
    }
    let teams = teams(groups, grant, current);
    let old: BTreeMap<&str, &Json> = current.iter().map(|e| (repo(e), e)).collect();
    let listed: BTreeSet<&str> = new.iter().map(repo).collect();
    let check = |before: Option<&Json>, after: Option<&Json>| {
        let (identity, change) = match (before, after) {
            (None, Some(after)) => (repo(after), Change::Add),
            (Some(before), Some(after)) if before == after => return None,
            (Some(before), Some(_)) => (repo(before), Change::Edit),
            (Some(before), None) => (repo(before), Change::Remove),
            (None, None) => return None,
        };
        let attributed = change == Change::Add && github_host.is_some_and(|h| on_host(identity, h));
        let owned_by = before.or(after).and_then(owner);
        let edited = |key: fn(&Json) -> Option<&str>| {
            change == Change::Edit && before.and_then(key) != after.and_then(key)
        };
        let reason = if !grant.reads(identity) && !attributed {
            Reason::OutsideGrant
        } else if attributed && (login.is_none() || after.and_then(added_by).is_none()) {
            Reason::Unattributed
        } else if owned_by.is_none() {
            Reason::Ownerless
        } else if edited(owner) {
            Reason::OwnerChange
        } else if github_host.is_some() && edited(added_by) {
            Reason::AddedByChange
        } else if !owned_by.is_some_and(|owner| teams.contains(&owner)) {
            Reason::NotYourTeam
        } else {
            return None;
        };
        Some(Refusal {
            identity: identity.to_string(),
            change,
            reason,
        })
    };
    let changed = (new.iter()).map(|e| check(old.get(repo(e)).copied(), Some(e)));
    let removed = (current.iter())
        .filter(|e| !listed.contains(repo(e)))
        .map(|e| check(Some(e), None));
    changed.chain(removed).flatten().collect()
}

/// The version a team operator's save stores: `current` in its order, each entry `grant` reads
/// replaced by its form in `submitted` or dropped when `submitted` omits it, each entry outside
/// the grant kept as it is, then the entries new to the workspace in `submitted` order. Under the
/// GitHub grant provider (`github_host`), each of those additions is attributed: on its host,
/// `added_by` is `login` whatever the submitted text held, absent without a valid login; off it,
/// absent.
/// Edited entries keep their submitted form. Run with `current` the entries the grant reads, it
/// is the change `authorize_entries` authorizes; with the whole current version, once that
/// refused nothing, the version stored.
pub fn compose_entries(
    grant: &Grant,
    login: Option<&str>,
    github_host: Option<&str>,
    current: &[Json],
    submitted: &[Json],
) -> Vec<Json> {
    let by_repo: BTreeMap<&str, &Json> = submitted.iter().map(|e| (repo(e), e)).collect();
    let present: BTreeSet<&str> = current.iter().map(repo).collect();
    let kept = current.iter().filter_map(|e| match grant.reads(repo(e)) {
        true => by_repo.get(repo(e)).copied().cloned(),
        false => Some(e.clone()),
    });
    let login = login.filter(|l| crate::manifest::added_by_problem(l).is_none());
    let added = (submitted.iter())
        .filter(|e| !present.contains(repo(e)))
        .map(|e| match github_host {
            Some(host) => attributed(e, login.filter(|_| on_host(repo(e), host))),
            None => e.clone(),
        });
    kept.chain(added).collect()
}

/// `entry` with `added_by` set to `login`, or without it when there is none.
fn attributed(entry: &Json, login: Option<&str>) -> Json {
    let mut entry = entry.clone();
    if let Json::Object(fields) = &mut entry {
        match login {
            Some(login) => fields.insert("added_by".into(), login.into()),
            None => fields.remove("added_by"),
        };
    }
    entry
}
