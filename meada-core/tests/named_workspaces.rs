//! The Y/A/B matrix of ADR 0020 and the contracts of ADR 0021 as an executable table
//! (`tests/fixtures/named-workspaces.json`, #340). No runtime exists yet, so this file holds a
//! small reference of the rules the ADRs state and checks every row of the table against it, and
//! that the table covers what the spec requires. The pure rules of the creation, discovery,
//! answer and sponsor rows are `meada_core::named`'s since #342, the functions the console and the
//! browser call; the binary's public-process tests and the front end's WebAssembly tests read the
//! same rows.

use std::collections::{BTreeMap, BTreeSet};

use meada_core::authorize::Grant;
use meada_core::named;
use serde::Deserialize;

#[derive(Deserialize)]
struct Table {
    accepted_roles: Vec<String>,
    callers: BTreeMap<String, Caller>,
    workspaces: BTreeMap<String, Workspace>,
    creation: Vec<Creation>,
    stale: Vec<Creation>,
    discovery: Vec<Discovery>,
    authority: Vec<Authority>,
    queries: Vec<Query>,
    links: Vec<Link>,
    revocation: Vec<Revocation>,
    sponsor: Vec<Sponsor>,
    membership: Membership,
    lifecycle: Vec<Lifecycle>,
}

#[derive(Deserialize, Clone)]
struct Caller {
    login: Option<String>,
    rows: Vec<Row>,
    /// Identities a claims grant reads: reading only, never creation (ADR 0020).
    #[serde(default)]
    claims_grant: Vec<String>,
    #[serde(default)]
    platform_administrator: bool,
}

#[derive(Deserialize, Clone)]
struct Row {
    identity: String,
    role: String,
}

#[derive(Deserialize, Clone)]
struct Workspace {
    entries: Vec<String>,
    operators: Vec<String>,
}

#[derive(Deserialize)]
struct Creation {
    caller: String,
    select: Vec<String>,
    allowed: bool,
    #[serde(default)]
    named: Vec<String>,
    hidden: Option<usize>,
    pending: Option<String>,
}

#[derive(Deserialize)]
struct Discovery {
    caller: String,
    workspace: String,
    visible: bool,
}

#[derive(Deserialize)]
struct Authority {
    caller: String,
    workspace: String,
    may_mutate: bool,
}

#[derive(Deserialize)]
struct Query {
    caller: String,
    workspace: String,
    identity: String,
    answers: bool,
}

#[derive(Deserialize)]
struct Link {
    caller: String,
    workspace: String,
    from: String,
    to: String,
    shown: bool,
}

#[derive(Deserialize)]
struct Revoke {
    caller: String,
    identity: String,
}

#[derive(Deserialize)]
struct Revocation {
    name: String,
    revoke: Revoke,
    workspace: String,
    expect: Vec<Expect>,
}

#[derive(Deserialize)]
struct Expect {
    caller: String,
    visible: bool,
    readable: Option<Vec<String>>,
    hidden: Option<usize>,
    may_add: Option<Vec<String>>,
    may_remove: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Sponsor {
    name: String,
    workspace: String,
    operators: Option<Vec<String>>,
    revoke: Revoke,
    excluded: Vec<String>,
}

#[derive(Deserialize)]
struct Lifecycle {
    name: String,
    caller: String,
    workspace: String,
    revoke: Option<Revoke>,
    submit: Vec<String>,
    stored: Option<Vec<String>>,
    rechecked: Option<Vec<String>>,
    allowed: Option<bool>,
    hidden: Option<usize>,
    operates: Option<bool>,
}

#[derive(Deserialize)]
struct Membership {
    named: Vec<String>,
    members: Vec<String>,
    bundles: BTreeMap<String, usize>,
}

fn table() -> Table {
    serde_json::from_str(include_str!("../../tests/fixtures/named-workspaces.json")).unwrap()
}

/// What creating a workspace from a selection gives: allowed, or pending on a condition of the
/// platform or the caller; else the refused identities the caller's grant reads and the count of
/// the rest.
#[derive(Debug, PartialEq)]
struct Outcome {
    allowed: bool,
    pending: Option<&'static str>,
    named: Vec<String>,
    hidden: usize,
}

impl Outcome {
    fn pending(reason: &'static str) -> Self {
        Outcome {
            allowed: false,
            pending: Some(reason),
            named: Vec::new(),
            hidden: 0,
        }
    }
}

/// The rules of ADR 0020 and ADR 0021 over a table's callers, at one freshness.
#[derive(Clone)]
struct World<'a> {
    table: &'a Table,
    callers: BTreeMap<String, Caller>,
    fresh: bool,
}

impl<'a> World<'a> {
    fn new(table: &'a Table, fresh: bool) -> Self {
        World {
            table,
            callers: table.callers.clone(),
            fresh,
        }
    }

    fn caller(&self, name: &str) -> &Caller {
        &self.callers[name]
    }

    /// The identities the caller's login holds an accepted role on, in a fresh set: what makes a
    /// repository eligible for creation. Nothing else does (a claims grant does not).
    fn eligible(&self, caller: &Caller) -> BTreeSet<String> {
        if !self.fresh || caller.login.is_none() {
            return BTreeSet::new();
        }
        let accepted = |row: &&Row| self.table.accepted_roles.contains(&row.role);
        caller
            .rows
            .iter()
            .filter(accepted)
            .map(|r| r.identity.clone())
            .collect()
    }

    /// What the caller reads: eligible identities and the claims grant. The platform
    /// administrator's graph answers are bounded by its grant like anyone's.
    fn grant(&self, caller: &Caller) -> BTreeSet<String> {
        let mut grant = self.eligible(caller);
        grant.extend(caller.claims_grant.iter().cloned());
        grant
    }

    fn create(&self, name: &str, select: &[String]) -> Outcome {
        let caller = self.caller(name);
        if !self.fresh {
            return Outcome::pending("stale_set");
        }
        if caller.platform_administrator {
            return Outcome {
                allowed: true,
                pending: None,
                named: Vec::new(),
                hidden: 0,
            };
        }
        if caller.login.is_none() {
            return Outcome::pending("missing_login");
        }
        let (eligible, grant) = (self.eligible(caller), self.grant(caller));
        let got = named::creation(select, &eligible, &Grant::Only(grant));
        Outcome {
            allowed: got.allowed,
            pending: None,
            named: got.named,
            hidden: got.hidden,
        }
    }

    fn operates(&self, name: &str, workspace: &Workspace) -> bool {
        self.caller(name).platform_administrator || workspace.operators.iter().any(|o| o == name)
    }

    /// ADR 0020: a non-operator sees a workspace only when its grant reads every entry.
    fn visible(&self, name: &str, workspace: &Workspace) -> bool {
        let grant = Grant::Only(self.grant(self.caller(name)));
        named::visible(self.operates(name, workspace), &workspace.entries, &grant)
    }

    fn readable(&self, name: &str, workspace: &Workspace) -> Vec<String> {
        let grant = Grant::Only(self.grant(self.caller(name)));
        named::readable(&workspace.entries, &grant)
    }

    fn answers(&self, name: &str, workspace: &Workspace, identity: &str) -> bool {
        let grant = Grant::Only(self.grant(self.caller(name)));
        let operates = self.operates(name, workspace);
        named::answers(operates, &workspace.entries, &grant, identity)
    }

    fn shown(&self, name: &str, workspace: &Workspace, from: &str, to: &str) -> bool {
        let grant = Grant::Only(self.grant(self.caller(name)));
        let operates = self.operates(name, workspace);
        named::shown(operates, &workspace.entries, &grant, from, to)
    }

    /// The entries no operator holds an accepted role on.
    fn excluded(&self, workspace: &Workspace, operators: &[String]) -> Vec<String> {
        let eligible: Vec<BTreeSet<String>> = (operators.iter())
            .map(|op| self.eligible(self.caller(op)))
            .collect();
        named::unsponsored(&workspace.entries, &eligible)
    }

    fn without(&self, revoke: &Revoke) -> World<'a> {
        let mut world = self.clone();
        let caller = world.callers.get_mut(&revoke.caller).unwrap();
        caller.rows.retain(|r| r.identity != revoke.identity);
        caller.claims_grant.retain(|i| *i != revoke.identity);
        world
    }
}

fn sorted(items: &[String]) -> Vec<String> {
    let mut items = items.to_vec();
    items.sort();
    items
}

#[test]
fn every_creation_row_gives_the_answer_the_matrix_lists() {
    let table = table();
    let world = World::new(&table, true);
    for row in &table.creation {
        let got = world.create(&row.caller, &row.select);
        let label = format!("{} selects {:?}", row.caller, row.select);
        assert_eq!(got.allowed, row.allowed, "{label}");
        assert_eq!(got.pending, row.pending.as_deref(), "{label}");
        assert_eq!(sorted(&got.named), sorted(&row.named), "{label}: named");
        if let Some(expected) = row.hidden {
            assert_eq!(got.hidden, expected, "{label}: hidden count");
        }
    }
}

#[test]
fn a_stale_set_is_pending_for_everyone_including_the_administrator() {
    let table = table();
    let world = World::new(&table, false);
    for row in &table.stale {
        let got = world.create(&row.caller, &row.select);
        assert!(!row.allowed && !got.allowed, "{}", row.caller);
        assert_eq!(got.pending, row.pending.as_deref(), "{}", row.caller);
    }
}

#[test]
fn discovery_is_all_or_nothing_for_a_non_operator() {
    let table = table();
    let world = World::new(&table, true);
    for row in &table.discovery {
        let workspace = &table.workspaces[&row.workspace];
        let visible = world.visible(&row.caller, workspace);
        assert_eq!(
            visible, row.visible,
            "{} sees {}",
            row.caller, row.workspace
        );
    }
}

#[test]
fn only_an_operator_or_the_administrator_mutates_a_workspace() {
    let table = table();
    let world = World::new(&table, true);
    for row in &table.authority {
        let workspace = &table.workspaces[&row.workspace];
        let may = world.operates(&row.caller, workspace);
        assert_eq!(may, row.may_mutate, "{} on {}", row.caller, row.workspace);
    }
}

#[test]
fn sharing_y_and_following_a_link_grant_nothing() {
    let table = table();
    let world = World::new(&table, true);
    for row in &table.queries {
        let workspace = &table.workspaces[&row.workspace];
        let answers = world.answers(&row.caller, workspace, &row.identity);
        assert_eq!(
            answers, row.answers,
            "{} asks {} in {}",
            row.caller, row.identity, row.workspace
        );
    }
    for row in &table.links {
        let workspace = &table.workspaces[&row.workspace];
        let shown = world.shown(&row.caller, workspace, &row.from, &row.to);
        assert_eq!(
            shown, row.shown,
            "{}: {} to {} in {}",
            row.caller, row.from, row.to, row.workspace
        );
    }
}

#[test]
fn revocation_after_creation_follows_the_table() {
    let table = table();
    let before = World::new(&table, true);
    for case in &table.revocation {
        let world = before.without(&case.revoke);
        let workspace = &table.workspaces[&case.workspace];
        for expect in &case.expect {
            let label = format!("{}: {}", case.name, expect.caller);
            assert_eq!(
                world.visible(&expect.caller, workspace),
                expect.visible,
                "{label}"
            );
            if !expect.visible {
                continue;
            }
            let readable = world.readable(&expect.caller, workspace);
            if let Some(expected) = &expect.readable {
                assert_eq!(&readable, expected, "{label}: readable");
            }
            if let Some(expected) = expect.hidden {
                assert_eq!(
                    workspace.entries.len() - readable.len(),
                    expected,
                    "{label}: hidden"
                );
            }
            let caller = world.caller(&expect.caller);
            if let Some(expected) = &expect.may_add {
                let added: Vec<String> = world.eligible(caller).into_iter().collect();
                assert_eq!(&added, expected, "{label}: may add");
            }
            if let Some(expected) = &expect.may_remove {
                assert_eq!(&readable, expected, "{label}: may remove");
            }
        }
    }
}

#[test]
fn a_sponsor_less_entry_is_excluded_before_the_fetch() {
    let table = table();
    let before = World::new(&table, true);
    for case in &table.sponsor {
        let world = before.without(&case.revoke);
        let workspace = &table.workspaces[&case.workspace];
        let operators = case.operators.as_ref().unwrap_or(&workspace.operators);
        let excluded = world.excluded(workspace, operators);
        assert_eq!(excluded, case.excluded, "{}", case.name);
    }
}

#[test]
fn shared_ingestion_holds_one_entry_per_identity() {
    let table = table();
    let mut members = BTreeSet::new();
    let mut entries = 0;
    for name in &table.membership.named {
        for entry in &table.workspaces[name].entries {
            members.insert(entry.clone());
            entries += 1;
        }
    }
    let listed: Vec<String> = members.into_iter().collect();
    assert_eq!(listed, table.membership.members);
    assert!(entries > listed.len(), "the table overlaps workspaces");
    for member in &listed {
        assert_eq!(
            table.membership.bundles[member], 1,
            "{member} is indexed once"
        );
    }
}

#[test]
fn provenance_decides_nothing() {
    // A role held directly and the same role through a group are the same row: every outcome of
    // alice (groups) equals dave's (direct) on every selection and workspace.
    let table = table();
    let world = World::new(&table, true);
    let selections: [&[&str]; 7] = [
        &["Y"],
        &["A"],
        &["B"],
        &["Y", "A"],
        &["Y", "B"],
        &["A", "B"],
        &["Y", "A", "B"],
    ];
    for select in selections {
        let select: Vec<String> = select.iter().map(|s| s.to_string()).collect();
        let (a, d) = (
            world.create("alice", &select),
            world.create("dave", &select),
        );
        assert_eq!(a, d, "{select:?}");
    }
    for (name, workspace) in &table.workspaces {
        assert_eq!(
            world.visible("alice", workspace),
            world.visible("dave", workspace),
            "{name}"
        );
    }
}

#[test]
fn the_table_covers_what_the_spec_requires() {
    let table = table();
    let create = |caller: &str, select: &[&str]| {
        table.creation.iter().find(|r| {
            r.caller == caller
                && r.select
                    .iter()
                    .map(String::as_str)
                    .eq(select.iter().copied())
        })
    };
    let allowed = |caller, select: &[&str]| create(caller, select).is_some_and(|r| r.allowed);
    let refused = |caller, select: &[&str]| create(caller, select).is_some_and(|r| !r.allowed);
    // A-only creates Y+A and is refused Y+B; B-only the reverse; both groups the union.
    assert!(allowed("alice", &["Y", "A"]) && refused("alice", &["Y", "B"]));
    assert!(allowed("bob", &["Y", "B"]) && refused("bob", &["Y", "A"]));
    assert!(allowed("carol", &["Y", "A", "B"]));
    // Granting Y alone grants neither.
    assert!(
        allowed("yoda", &["Y"]) && refused("yoda", &["Y", "A"]) && refused("yoda", &["Y", "B"])
    );
    // Direct and group-derived roles, a rejected role, a claims grant, no login, a stale set and
    // the administrator are all exercised.
    assert!(allowed("dave", &["Y", "A"]));
    assert!(refused("zed", &["Y", "A"]) && refused("claims", &["Y", "A"]));
    assert!(
        table
            .creation
            .iter()
            .any(|r| r.pending.as_deref() == Some("missing_login"))
    );
    assert!(
        table
            .stale
            .iter()
            .any(|r| r.pending.as_deref() == Some("stale_set"))
    );
    assert!(allowed("root", &["Y", "A", "B"]));
    // The creator versus a group peer: the peer views and mutates nothing.
    let peer = |v| {
        table
            .discovery
            .iter()
            .any(|r| r.caller == "erin" && r.workspace == "w_ya" && r.visible == v)
    };
    assert!(peer(true));
    assert!(
        table
            .authority
            .iter()
            .any(|r| r.caller == "erin" && !r.may_mutate)
    );
    // A partial reader sees nothing, and revocation is exercised for the creator and the peer.
    assert!(
        table
            .discovery
            .iter()
            .any(|r| r.caller == "yoda" && r.workspace == "w_ya" && !r.visible)
    );
    assert!(table.revocation.len() >= 3 && table.sponsor.len() >= 2);
    // Every name a row uses exists.
    for row in &table.discovery {
        assert!(
            table.callers.contains_key(&row.caller)
                && table.workspaces.contains_key(&row.workspace)
        );
    }
}

fn entries(items: &[String]) -> Vec<serde_json::Value> {
    items
        .iter()
        .map(|i| serde_json::json!({"repo": i}))
        .collect()
}

#[test]
fn a_save_keeps_hidden_entries_and_rechecks_only_what_it_adds_or_changes() {
    let table = table();
    let before = World::new(&table, true);
    for row in &table.lifecycle {
        let world = match &row.revoke {
            Some(revoke) => before.without(revoke),
            None => before.clone(),
        };
        let workspace = &table.workspaces[&row.workspace];
        let caller = world.caller(&row.caller);
        let operates = world.operates(&row.caller, workspace);
        if let Some(expected) = row.operates {
            assert_eq!(operates, expected, "{}", row.name);
            continue;
        }
        let current = entries(&workspace.entries);
        let submitted = entries(&row.submit);
        // The administrator submits the whole manifest; an operator submits what it reads and
        // the version stored keeps the rest in place.
        let (grant, stored) = match caller.platform_administrator {
            true => (Grant::All, submitted.clone()),
            false => {
                let grant = Grant::Only(world.grant(caller));
                let stored = named::compose(&grant, &current, &submitted);
                (grant, stored)
            }
        };
        let identities = named::identities(&stored);
        assert_eq!(
            Some(&identities),
            row.stored.as_ref(),
            "{}: stored",
            row.name
        );
        let rechecked = named::changed(&current, &stored);
        assert_eq!(
            Some(&rechecked),
            row.rechecked.as_ref(),
            "{}: rechecked",
            row.name
        );
        // The catalog holds every table repository: the administrator's eligible set.
        let eligible: BTreeSet<String> = match caller.platform_administrator {
            true => table_identities(&table),
            false => world.eligible(caller),
        };
        let decided = named::creation(&rechecked, &eligible, &grant);
        assert_eq!(Some(decided.allowed), row.allowed, "{}: allowed", row.name);
        if let Some(hidden) = row.hidden {
            assert_eq!(decided.hidden, hidden, "{}: hidden", row.name);
        }
    }
}

fn table_identities(table: &Table) -> BTreeSet<String> {
    (table.workspaces.values())
        .flat_map(|w| w.entries.iter().cloned())
        .collect()
}
