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

const ACCOUNTS: &str = "accounts";

pub(in crate::ui) fn is_administration(route: &str) -> bool {
    route == ACCOUNTS || named(route).is_some()
}

fn named(route: &str) -> Option<&str> {
    route.strip_prefix("accounts/")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    username: String,
    role: Role,
    groups: Option<Vec<String>>,
    password: String,
    change_required: Option<bool>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Role {
        role: Role,
    },
    Groups {
        groups: Vec<String>,
    },
    Suspend {},
    Reactivate {},
    ResetPassword {
        password: String,
        change_required: Option<bool>,
    },
    RemoveGithub {},
}

impl Action {
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

fn recorded_groups(groups: &[String]) -> String {
    json!(groups).to_string()
}

struct Recorded<'a> {
    pending: Pending,
    action: &'a str,
    account: &'a str,
}

impl Console {
    pub(in crate::ui) fn has_administration(&self, route: &str) -> bool {
        let local = self.accounts.as_ref();
        local.is_some_and(|local| !local.break_glass) && is_administration(route)
    }

    pub(in crate::ui) fn administration_refused(
        &self,
        session: &Session,
        route: Option<&str>,
        method: &Method,
    ) -> Option<Box<Answer>> {
        let route = route.filter(|r| self.has_administration(r))?;
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
            _ => None,
        }
    }

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

    fn change_account(
        &self,
        local: &Local,
        username: &str,
        body: &Body,
        session: &Session,
    ) -> Answer {
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
