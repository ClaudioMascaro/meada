//! Account administration (#557, ADR 0024): the global operator's routes over the accounts
//! table, whatever `[ui] operations` says, so a read-only console can still have viewers.
//!
//! - `GET accounts`: every account (`store::list`), by username, never a hash.
//! - `POST accounts`: `{username, role, groups, password, change_required}` creates one, its
//!   temporary password through the policy, a change required unless the body turns it off,
//!   `groups` none when absent; a malformed username is a 422 `invalid_username`, a group that is
//!   no owner name a 422 `invalid_group`, a taken username a 409 `username_taken`.
//! - `POST accounts/<username>`: one action, `{"action": "role", "role"}`, `{"action": "groups",
//!   "groups"}` (#558: each an owner name, ADR 0015, sorted without repeats), `suspend`,
//!   `reactivate`, or `{"action": "reset_password", "password", "change_required"}`, which also
//!   clears a lock and requires a change unless the body turns it off, or `{"action":
//!   "remove_github"}` (#561), which removes its verified GitHub login. An unknown account is a
//!   404; a suspension or a demotion of the last active operator a 409 `last_operator`, decided
//!   under the schema's advisory lock (`store::change`). No route deletes or renames an account.
//!
//! Anyone but the global operator gets a 403 before the body is read
//! (`Console::administration_refused`), recorded for a write as an operation's role refusal is:
//! `account.create`, or the route's own `account` for a change whose action is still unread. A
//! change passes its body's checks
//! (400, 413, 422, not recorded), then is recorded `pending` (`account.create`, `account.role`,
//! `account.groups`, `account.suspend`, `account.reactivate`, `account.reset_password`,
//! `account.remove_github`, naming the
//! account and the fields changed, the groups as a JSON array in a string, never a password)
//! before the table is touched, and finished with its outcome;
//! a record that cannot be inserted is a 503 `audit_unavailable` and nothing changes.

use hyper::{Method, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Answer, Body, Console, Local, Session, error, json_answer, parsed, write};
use super::{bad_row, hashed, refused_by, refused_line, unavailable, unreachable};
use crate::accounts::store::{self, Change, Changed, Listed};
use crate::accounts::{self, Role};
use crate::audit::{Outcome, Pending};
use crate::msg::Msg;

// ponytail: the most bytes of an account creation's or change's body: a username, a password
// of at most 128 characters and a list of groups, each escaped, with room to spare; past it a 413.
pub(in crate::ui) const ADMIN_BODY_MAX: usize = 4 << 10;

/// The list and the creation.
const ACCOUNTS: &str = "accounts";

/// Whether `route` is one of account administration's.
pub(in crate::ui) fn is_administration(route: &str) -> bool {
    route == ACCOUNTS || named(route).is_some()
}

/// The account a change's route names, as written.
fn named(route: &str) -> Option<&str> {
    route.strip_prefix("accounts/")
}

/// The body of `POST accounts`.
// No `Debug`: the body holds a password.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    username: String,
    role: Role,
    groups: Option<Vec<String>>,
    password: String,
    change_required: Option<bool>,
}

/// The body of `POST accounts/<username>`: one action and its fields.
// No `Debug`: a reset holds a password.
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Role {
        role: Role,
    },
    Groups {
        groups: Vec<String>,
    },
    // Empty structs rather than units, so a field beside them is refused as for the others.
    Suspend {},
    Reactivate {},
    ResetPassword {
        password: String,
        change_required: Option<bool>,
    },
    /// The account's verified GitHub login removed (#561).
    RemoveGithub {},
}

impl Action {
    /// The action its audit record names.
    fn recorded(&self) -> &'static str {
        match self {
            Action::Role { .. } => "account.role",
            Action::Groups { .. } => "account.groups",
            Action::Suspend {} => "account.suspend",
            Action::Reactivate {} => "account.reactivate",
            Action::ResetPassword { .. } => "account.reset_password",
            Action::RemoveGithub {} => "account.remove_github",
        }
    }

    /// The audit message of the action accepted.
    fn accepted(&self) -> String {
        match self {
            Action::Role { .. } => Msg::AccountsRoleChanged.text(),
            Action::Groups { .. } => Msg::AccountsGroupsChanged.text(),
            Action::Suspend {} => Msg::AccountsSuspended.text(),
            Action::Reactivate {} => Msg::AccountsReactivated.text(),
            Action::ResetPassword { .. } => Msg::AccountsPasswordReset.text(),
            Action::RemoveGithub {} => Msg::AccountsGithubRemoved.text(),
        }
    }
}

/// An account as the API shows it: never its hash.
fn document(account: &Listed) -> Value {
    json!({
        "username": account.username,
        "role": account.role.name(),
        "groups": account.groups,
        "state": account.state(),
        "locked_until": account.locked_until,
        "change_required": account.change_required,
        "password_changed": account.password_changed,
        "created": account.created,
        "created_by": account.created_by,
        "github_login": account.github_login,
    })
}

/// `groups` sorted without repeats, or the 422 `invalid_group` when one is no name an entry's
/// owner may be (ADR 0015), so a group always means what an owner of the same name does.
fn checked_groups(mut groups: Vec<String>) -> std::result::Result<Vec<String>, Box<Answer>> {
    if !accounts::groups_ok(&groups) {
        let message = Msg::AccountsGroupInvalid.text();
        let answer = error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_group", &message);
        return Err(Box::new(answer));
    }
    groups.sort();
    groups.dedup();
    Ok(groups)
}

/// `groups` as an audit argument: a JSON array in a string, since every argument is a string.
fn recorded_groups(groups: &[String]) -> String {
    json!(groups).to_string()
}

/// A change's pending record, and what its stderr line names.
struct Recorded<'a> {
    pending: Pending,
    action: &'a str,
    account: &'a str,
}

impl Console {
    /// Whether `route` is one of this console's account administration routes: none without
    /// `[accounts]`, and none under break glass, where account administration does not exist
    /// (#563), so its routes are the 404 of any unknown route.
    pub(in crate::ui) fn has_administration(&self, route: &str) -> bool {
        let local = self.accounts.as_ref();
        local.is_some_and(|local| !local.break_glass) && is_administration(route)
    }

    /// The role check of account administration, before the body is read: `None` for any other
    /// route (`has_administration`) or for the global operator; else the 403, with, for a write,
    /// its stderr line and its record (`account.create`, or `account` for a change, naming the
    /// account when its route names a valid username).
    pub(in crate::ui) fn administration_refused(
        &self,
        session: &Session,
        route: Option<&str>,
        method: &Method,
    ) -> Option<Box<Answer>> {
        let route = route.filter(|r| self.has_administration(r))?;
        // A local account's own role: never an identity provider's operator (ADR 0024).
        if session.operator && session.account {
            return None;
        }
        let message = Msg::AccountsOperatorRequired.text();
        if !matches!(*method, Method::GET | Method::HEAD) {
            let (action, account) = match named(route) {
                None => ("account.create", None),
                Some(name) => ("account", Some(name).filter(|n| accounts::username_ok(n))),
            };
            let shown = account.unwrap_or("-");
            write::audit(session.subject.as_deref(), None, action, shown, &message);
            let arguments = match account {
                Some(account) => json!({"account": account}),
                None => json!({}),
            };
            let actor = write::actor(session, None);
            self.audit.refused(&actor, action, &arguments, &message);
        }
        Some(Box::new(error(
            StatusCode::FORBIDDEN,
            "forbidden",
            &message,
        )))
    }

    /// The answer of an account administration route, `None` for any other. The session is the
    /// global operator's (`administration_refused` passed); checked again here, so no route
    /// answers if a dispatch ever skips that check.
    pub(in crate::ui) fn administer(
        &self,
        local: &Local,
        method: &Method,
        route: &str,
        body: &Body,
        session: &Session,
    ) -> Option<Answer> {
        if !self.has_administration(route) {
            return None;
        }
        if !(session.operator && session.account) {
            let message = Msg::AccountsOperatorRequired.text();
            return Some(error(StatusCode::FORBIDDEN, "forbidden", &message));
        }
        let read = matches!(*method, Method::GET | Method::HEAD);
        match (named(route), read, *method == Method::POST) {
            (None, true, _) => Some(self.list_accounts(local)),
            (None, _, true) => Some(self.create_account(local, body, session)),
            (Some(username), _, true) => Some(self.change_account(local, username, body, session)),
            // No other method: nothing deletes or renames an account.
            _ => None,
        }
    }

    /// `GET accounts`: every account by username; a row that fails its checks is left out, with
    /// one line naming the table.
    fn list_accounts(&self, local: &Local) -> Answer {
        match store::list(&local.postgres) {
            Ok((listed, skipped)) => {
                if skipped > 0 {
                    bad_row(local);
                }
                let accounts: Vec<Value> = listed.iter().map(document).collect();
                json_answer(StatusCode::OK, &json!({"accounts": accounts}))
            }
            Err(reason) => unreachable(&reason),
        }
    }

    /// The pending record of `action` on `account` with `arguments` by `session`, or the 503
    /// `audit_unavailable` (nothing changes).
    fn account_pending<'a>(
        &self,
        session: &Session,
        action: &'a str,
        account: &'a str,
        arguments: &Value,
    ) -> std::result::Result<Recorded<'a>, Box<Answer>> {
        let actor = write::actor(session, None);
        match self.audit.pending(&actor, action, arguments) {
            Ok(pending) => Ok(Recorded {
                pending,
                action,
                account,
            }),
            Err(reason) => {
                let subject = session.subject.as_deref();
                Err(Box::new(unavailable(&reason, subject, action, account)))
            }
        }
    }

    /// Finishes `recorded` with what the store answered: the account on success, the 404, the
    /// 409 or the 503 otherwise, each with its stderr line.
    fn finished(
        &self,
        local: &Local,
        session: &Session,
        recorded: Recorded,
        done: std::result::Result<Changed, String>,
        accepted: String,
    ) -> Answer {
        let Recorded {
            pending,
            action,
            account,
        } = recorded;
        let subject = session.subject.as_deref();
        let (status, code, message) = match done {
            Ok(Changed::Done(listed)) => {
                self.audit
                    .finish(pending, Outcome::Accepted, &accepted, None);
                write::audit(subject, None, action, account, "accepted");
                if listed.is_none() {
                    bad_row(local);
                }
                let document = listed.as_ref().map(document);
                return json_answer(StatusCode::OK, &json!({"account": document}));
            }
            Ok(Changed::Taken) => (
                StatusCode::CONFLICT,
                "username_taken",
                Msg::AccountsTaken.text(),
            ),
            Ok(Changed::LastOperator) => (
                StatusCode::CONFLICT,
                "last_operator",
                Msg::AccountsLastOperator.text(),
            ),
            // Only a verification sets a login: no administration change meets these.
            Ok(Changed::Suspended) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                Msg::AccountsGithubForbidden.text(),
            ),
            Ok(Changed::LoginTaken) => (
                StatusCode::CONFLICT,
                "login_taken",
                Msg::AccountsLoginTaken.text(),
            ),
            // A row that fails its checks admits nothing, as no row does.
            Ok(Changed::Unknown | Changed::Bad) => (
                StatusCode::NOT_FOUND,
                "not_found",
                Msg::AccountsUnknown.text(),
            ),
            Err(reason) => {
                let answer = unreachable(&reason);
                let message = Msg::AccountsUnavailable.text();
                self.audit.finish(pending, Outcome::Failed, &message, None);
                write::audit(subject, None, action, account, &message);
                return answer;
            }
        };
        self.audit.finish(pending, Outcome::Refused, &message, None);
        refused_line(subject, action, account, code);
        error(status, code, &message)
    }

    /// `POST accounts`: a new account, created by the session's subject.
    fn create_account(&self, local: &Local, body: &Body, session: &Session) -> Answer {
        let create = match parsed::<Create>(body, ADMIN_BODY_MAX, Msg::AccountsCreateBody) {
            Ok(create) => create,
            Err(refused) => return *refused,
        };
        if !accounts::username_ok(&create.username) {
            let message = Msg::AccountsUsernameInvalid.text();
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_username",
                &message,
            );
        }
        let groups = match checked_groups(create.groups.unwrap_or_default()) {
            Ok(groups) => groups,
            Err(refused) => return *refused,
        };
        if let Err(rule) = accounts::policy(&create.password, &create.username) {
            return refused_by(rule);
        }
        let hash = match hashed(local, &create.password) {
            Ok(hash) => hash,
            Err(refused) => return *refused,
        };
        let change_required = create.change_required.unwrap_or(true);
        let username = create.username.as_str();
        let action = "account.create";
        let arguments = json!({
            "account": username,
            "role": create.role.name(),
            "groups": recorded_groups(&groups),
            "change_required": change_required.to_string(),
        });
        let recorded = match self.account_pending(session, action, username, &arguments) {
            Ok(recorded) => recorded,
            Err(refused) => return *refused,
        };
        let creator = session.subject.as_deref().unwrap_or_default();
        let done = store::create(
            &local.postgres,
            username,
            create.role,
            &groups,
            &hash,
            change_required,
            creator,
        );
        let accepted = Msg::AccountsCreated.text();
        self.finished(local, session, recorded, done, accepted)
    }

    /// `POST accounts/<username>`: one action on the account `username`.
    fn change_account(
        &self,
        local: &Local,
        username: &str,
        body: &Body,
        session: &Session,
    ) -> Answer {
        // A route naming no possible account (a rename's `accounts/<username>/<new>` among them)
        // is the 404 whatever its body.
        if !accounts::username_ok(username) {
            let message = Msg::AccountsUnknown.text();
            return error(StatusCode::NOT_FOUND, "not_found", &message);
        }
        let action = match parsed::<Action>(body, ADMIN_BODY_MAX, Msg::AccountsChangeBody) {
            Ok(action) => action,
            Err(refused) => return *refused,
        };
        let mut arguments = json!({"account": username});
        let change = match &action {
            Action::Role { role } => {
                arguments["role"] = json!(role.name());
                Change::Role(*role)
            }
            Action::Groups { groups } => {
                let groups = match checked_groups(groups.clone()) {
                    Ok(groups) => groups,
                    Err(refused) => return *refused,
                };
                arguments["groups"] = json!(recorded_groups(&groups));
                Change::Groups(groups)
            }
            Action::Suspend {} => Change::Suspend,
            Action::Reactivate {} => Change::Reactivate,
            Action::RemoveGithub {} => Change::RemoveGithub,
            Action::ResetPassword {
                password,
                change_required,
            } => {
                if let Err(rule) = accounts::policy(password, username) {
                    return refused_by(rule);
                }
                let change_required = change_required.unwrap_or(true);
                arguments["change_required"] = json!(change_required.to_string());
                match hashed(local, password) {
                    Ok(hash) => Change::ResetPassword {
                        hash,
                        change_required,
                    },
                    Err(refused) => return *refused,
                }
            }
        };
        let recorded = match self.account_pending(session, action.recorded(), username, &arguments)
        {
            Ok(recorded) => recorded,
            Err(refused) => return *refused,
        };
        let done = store::change(&local.postgres, username, &change);
        if matches!(done, Ok(Changed::Bad)) {
            bad_row(local);
        }
        let accepted = action.accepted();
        self.finished(local, session, recorded, done, accepted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each body shape parses to its action, and nothing else does: an unknown action (a deletion
    /// or a rename among them), an unknown field, a missing one.
    #[test]
    fn a_change_is_one_action_with_its_own_fields() {
        let parse = |body: &str| serde_json::from_str::<Action>(body);
        assert!(matches!(
            parse(r#"{"action": "role", "role": "viewer"}"#),
            Ok(Action::Role { role: Role::Viewer })
        ));
        assert!(matches!(
            parse(r#"{"action": "groups", "groups": ["team-billing"]}"#),
            Ok(Action::Groups { groups }) if groups == ["team-billing"]
        ));
        assert!(matches!(
            parse(r#"{"action": "suspend"}"#),
            Ok(Action::Suspend {})
        ));
        assert!(matches!(
            parse(r#"{"action": "reactivate"}"#),
            Ok(Action::Reactivate {})
        ));
        assert!(matches!(
            parse(r#"{"action": "remove_github"}"#),
            Ok(Action::RemoveGithub {})
        ));
        assert!(matches!(
            parse(r#"{"action": "reset_password", "password": "p"}"#),
            Ok(Action::ResetPassword {
                change_required: None,
                ..
            })
        ));
        assert!(matches!(
            parse(r#"{"action": "reset_password", "password": "p", "change_required": false}"#),
            Ok(Action::ResetPassword {
                change_required: Some(false),
                ..
            })
        ));
        for refused in [
            r#"{"action": "delete"}"#,
            r#"{"action": "rename", "username": "eve"}"#,
            r#"{"action": "role", "role": "admin"}"#,
            r#"{"action": "role"}"#,
            r#"{"action": "suspend", "username": "eve"}"#,
            r#"{"action": "role", "role": "viewer", "groups": []}"#,
            r#"{"action": "groups"}"#,
            r#"{"action": "groups", "groups": "team-billing"}"#,
            r#"{"action": "groups", "groups": [], "role": "viewer"}"#,
            r#"{"action": "reset_password"}"#,
            r#"{"action": "remove_github", "login": "ana"}"#,
            r#"{"action": "github", "login": "ana"}"#,
            r#"{"role": "viewer"}"#,
        ] {
            assert!(parse(refused).is_err(), "{refused}");
        }
    }

    /// A creation takes a username, a role and a password, `groups` and `change_required`
    /// optional, and no other field.
    #[test]
    fn a_creation_takes_its_fields_and_no_other() {
        let parse = |body: &str| serde_json::from_str::<Create>(body);
        let created = parse(r#"{"username": "bo", "role": "operator", "password": "p"}"#).unwrap();
        assert_eq!(
            (
                created.username.as_str(),
                created.role,
                created.change_required
            ),
            ("bo", Role::Operator, None)
        );
        let grouped = r#"{"username": "bo", "role": "viewer", "groups": ["g"], "password": "p"}"#;
        assert_eq!(parse(grouped).unwrap().groups, Some(vec!["g".to_string()]));
        for refused in [
            r#"{"username": "bo", "role": "viewer"}"#,
            r#"{"username": "bo", "role": "root", "password": "p"}"#,
            r#"{"username": "bo", "role": "viewer", "password": "p", "hash": "h"}"#,
            r#"{"username": "bo", "role": "viewer", "password": "p", "groups": "g"}"#,
        ] {
            assert!(parse(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn only_the_list_and_its_accounts_are_administration_routes() {
        for route in ["accounts", "accounts/bo", "accounts/bo/rename"] {
            assert!(is_administration(route), "{route}");
        }
        for route in ["account/password", "accountsx", "login", "session"] {
            assert!(!is_administration(route), "{route}");
        }
    }

    /// Groups are owner names, kept sorted without repeats; one that is not refuses them all.
    #[test]
    fn groups_are_owner_names_sorted_without_repeats() {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let checked = checked_groups(names(&["team-web", "team-billing", "team-web"]));
        assert_eq!(checked.ok(), Some(names(&["team-billing", "team-web"])));
        assert_eq!(checked_groups(Vec::new()).ok(), Some(Vec::new()));
        let long = "g".repeat(meada_core::manifest::OWNER_MAX + 1);
        for refused in ["", " padded", "line\nbreak", long.as_str()] {
            let refused = checked_groups(names(&["team-web", refused])).err().unwrap();
            assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
        }
        assert_eq!(recorded_groups(&names(&["a", "b\"c"])), r#"["a","b\"c"]"#);
    }
}
