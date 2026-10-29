//! What a subject may change in a workspace version (ADR 0015): `authorize_entries`, run by the
//! server under the refresh lock and by the browser before a save, and `compose_entries`, the
//! version a team operator's save stores. Entries are a version document's `repositories`
//! (`repo`, `path`, `owner` when there is one, and the blocks), already validated. Pure: no
//! clock, network or file.

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
    /// The identity is outside the grant.
    OutsideGrant,
    /// The current entry has no owner (an edit or a removal), or an addition has none.
    Ownerless,
    /// The owner differs between the current and the new entry, cleared included.
    OwnerChange,
    /// The owner is not one of the subject's teams.
    NotYourTeam,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub identity: String,
    pub change: Change,
    pub reason: Reason,
}

fn repo(entry: &Json) -> &str {
    entry["repo"].as_str().unwrap_or_default()
}

fn owner(entry: &Json) -> Option<&str> {
    entry["owner"].as_str()
}

/// The subject's teams: its groups that own an entry of `current` whose identity `grant` reads.
fn teams<'a>(groups: &'a [String], grant: &Grant, current: &[Json]) -> Vec<&'a str> {
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
/// removed ones follow `current`. The global operator is refused nothing.
pub fn authorize_entries(
    role: Role,
    groups: &[String],
    grant: &Grant,
    current: &[Json],
    new: &[Json],
) -> Vec<Refusal> {
    if role == Role::Operator {
        return Vec::new();
    }
    let teams = teams(groups, grant, current);
    let old: BTreeMap<&str, &Json> = current.iter().map(|e| (repo(e), e)).collect();
    let kept: BTreeSet<&str> = new.iter().map(repo).collect();
    let check = |before: Option<&Json>, after: Option<&Json>| {
        let (identity, change) = match (before, after) {
            (None, Some(after)) => (repo(after), Change::Add),
            (Some(before), Some(after)) if before == after => return None,
            (Some(before), Some(_)) => (repo(before), Change::Edit),
            (Some(before), None) => (repo(before), Change::Remove),
            (None, None) => return None,
        };
        let held = before.or(after).and_then(owner);
        let reason = if !grant.reads(identity) {
            Reason::OutsideGrant
        } else if held.is_none() {
            Reason::Ownerless
        } else if change == Change::Edit && held != after.and_then(owner) {
            Reason::OwnerChange
        } else if !held.is_some_and(|owner| teams.contains(&owner)) {
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
        .filter(|e| !kept.contains(repo(e)))
        .map(|e| check(Some(e), None));
    changed.chain(removed).flatten().collect()
}

/// The version a team operator's save stores: `current` in its order, each entry `grant` reads
/// replaced by its form in `submitted` or dropped when `submitted` omits it, each entry outside
/// the grant kept as it is, then the entries new to the workspace in `submitted` order. Run only
/// once `authorize_entries`, given the current entries the grant reads, refused nothing.
pub fn compose_entries(grant: &Grant, current: &[Json], submitted: &[Json]) -> Vec<Json> {
    let by: BTreeMap<&str, &Json> = submitted.iter().map(|e| (repo(e), e)).collect();
    let held: BTreeSet<&str> = current.iter().map(repo).collect();
    let kept = current.iter().filter_map(|e| match grant.reads(repo(e)) {
        true => by.get(repo(e)).copied(),
        false => Some(e),
    });
    let added = submitted.iter().filter(|e| !held.contains(repo(e)));
    kept.chain(added).cloned().collect()
}
