//! Authorization and a team operator's save at the public core seam, driven by the shared table
//! Vitest also reads through the WebAssembly bundle.

use meada_core::authorize::{Grant, Refusal, Role, authorize_entries, compose_entries, role};
use serde::Deserialize;
use serde_json::{Value as Json, json};

#[derive(Deserialize)]
struct Case {
    name: String,
    role: Option<Role>,
    #[serde(default)]
    groups: Vec<String>,
    grant: Option<Grant>,
    #[serde(default)]
    current: Vec<Json>,
    #[serde(default)]
    new: Vec<Json>,
    refused: Option<Vec<Refusal>>,
    composed: Option<Vec<Json>>,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/workspace-validation.json"
    ))
    .unwrap()
}

#[test]
fn every_authorization_case_gives_the_refusals_the_table_lists() {
    let mut seen = 0;
    for case in cases() {
        let (Some(role), Some(grant), Some(refused)) = (case.role, &case.grant, &case.refused)
        else {
            continue;
        };
        let answer = authorize_entries(role, &case.groups, grant, &case.current, &case.new);
        assert_eq!(&answer, refused, "{}", case.name);
        let operator = authorize_entries(
            Role::Operator,
            &case.groups,
            grant,
            &case.current,
            &case.new,
        );
        assert!(operator.is_empty(), "{}: {operator:?}", case.name);
        seen += 1;
    }
    assert!(seen >= 20, "{seen} authorization cases");
}

#[test]
fn every_composition_case_stores_the_entries_the_table_lists() {
    let mut seen = 0;
    for case in cases() {
        let (Some(grant), Some(composed)) = (&case.grant, &case.composed) else {
            continue;
        };
        let answer = compose_entries(grant, &case.current, &case.new);
        assert_eq!(&answer, composed, "{}", case.name);
        seen += 1;
    }
    assert!(seen >= 2, "{seen} composition cases");
}

/// The role apply recomputes under the lock: a group owning an entry the grant reads.
#[test]
fn the_role_follows_the_entries_the_grant_reads() {
    let current = [json!({"repo": "a.com/a", "path": "a", "owner": "team-a"})];
    let groups = ["team-a".to_string()];
    let only = |id: &str| Grant::Only([id.to_string()].into());
    let team = Role::TeamOperator;
    assert_eq!(role(team, &groups, &only("a.com/a"), &current), team);
    assert_eq!(
        role(team, &groups, &only("a.com/b"), &current),
        Role::Viewer
    );
    assert_eq!(role(team, &[], &Grant::All, &current), Role::Viewer);
    assert_eq!(role(Role::Operator, &[], &Grant::All, &[]), Role::Operator);
    assert_eq!(role(Role::Viewer, &groups, &Grant::All, &current), team);
    let star: Grant = serde_json::from_value(json!("*")).unwrap();
    assert_eq!(star, Grant::All);
    assert!(serde_json::from_value::<Grant>(json!("a.com/a")).is_err());
}

/// An entry whose only difference is `added_by` is edited, so S3's rules apply to it (#330).
#[test]
fn an_added_by_change_alone_is_an_edit() {
    let entry = |added_by: Option<&str>| match added_by {
        Some(login) => {
            json!({"repo": "a.com/a", "path": "a", "owner": "team-a", "added_by": login})
        }
        None => json!({"repo": "a.com/a", "path": "a", "owner": "team-a"}),
    };
    let groups = ["team-a".to_string()];
    let team = Role::TeamOperator;
    for (before, after) in [
        (None, Some("ana")),
        (Some("ana"), None),
        (Some("ana"), Some("bia")),
    ] {
        let (current, new) = ([entry(before)], [entry(after)]);
        assert!(authorize_entries(team, &groups, &Grant::All, &current, &new).is_empty());
        let outside = Grant::Only(["a.com/b".to_string()].into());
        let refused = authorize_entries(team, &groups, &outside, &current, &new);
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].change, meada_core::authorize::Change::Edit);
        let other = ["team-b".to_string()];
        let refused = authorize_entries(team, &other, &Grant::All, &current, &new);
        assert_eq!(refused[0].change, meada_core::authorize::Change::Edit);
    }
}
