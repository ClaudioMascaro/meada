//! The console's side of local accounts (ADR 0024, feature `accounts`): with `[accounts]` and no
//! `[oidc]`, the console signs its own session tokens and admits them as bearers.
//!
//! - Start (`Local::start`): the signing key pair read once (`accounts::Config::keys`), then the
//!   default account created when the table is empty (`store::bootstrap`); either failing refuses
//!   the start.
//! - Admission (`Console::admit_local`): every route but the static files, `config` and `login`
//!   needs a session token (`Keys::verify`), else a 401 with `WWW-Authenticate: Bearer` before
//!   routing; a token carrying the change-required mark opens `account/password` alone, every
//!   other route a 403 `password_change_required`. The session is the token's: its role, its
//!   groups, and the grant the grant providers compute from them, every repository with no
//!   table.
//! - `POST login` (`Console::login`): `{username, password}`, one 401 `invalid_credentials` for
//!   an unknown username (hashed against `accounts::dummy`), a wrong password and a suspended
//!   account; `{token, expires_at, change_required}` on success.
//! - `POST account/password` (`Console::password`): `{current, new}`, the policy applied, the
//!   current one checked, a fresh session token without the mark on success.
//!
//! At most `HASHES_MAX` password hashes run at once; a further request is a 503 `busy` without
//! hashing. Every sign-in and every password change leaves an audit record before its token is
//! returned (`account.login`, `account.password`), and one stderr line; neither ever holds a
//! password, a hash or a token. A record that cannot be written is a 503 `audit_unavailable`
//! that gives no token.

use hyper::{HeaderMap, StatusCode, header};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Answer, Body, Console, Session, error, json_answer, write};
use crate::accounts::store::{self, Found};
use crate::accounts::{self, Keys, Role, Rule};
use crate::audit::{Actor, Outcome};
use crate::msg::Msg;

// ponytail: the most bytes of a sign-in's body: a username and a password of at most 128
// characters, each escaped, with room to spare; past it a 413.
pub(super) const LOGIN_MAX: usize = 4 << 10;
// ponytail: the most bytes of a password change's body, two passwords; past it a 413.
pub(super) const PASSWORD_BODY_MAX: usize = 4 << 10;
// ponytail: the most password hashes the console computes at once, each 19 MiB of memory; a
// further sign-in or password change is a 503 `busy` without hashing.
pub(super) const HASHES_MAX: usize = 4;

/// The routes of this module.
pub(super) const LOGIN: &str = "login";
pub(super) const PASSWORD: &str = "account/password";

/// What the console holds for local accounts.
pub(super) struct Local {
    keys: Keys,
    postgres: crate::postgres::Config,
    hashes: tokio::sync::Semaphore,
}

impl Local {
    /// The console's local accounts of `config`: the key pair read and checked, then the default
    /// account created when the table is empty, its initial password `admin123` with a change
    /// required, or the one the deployer set (which must pass the policy) without. Each problem
    /// is a key and its message.
    pub(super) fn start(
        config: &accounts::Config,
    ) -> std::result::Result<Self, crate::modules::Problem> {
        let keys = config.keys()?;
        let postgres =
            (config.postgres.clone()).expect("`Manifest::read` gives [accounts] its [postgres]");
        // Computed now, so the first unknown username costs what every other one does.
        let _ = accounts::dummy();
        let initial = || {
            let hashed = |password: &str| accounts::hash(password).map_err(|e| ("accounts", e));
            match config.admin_password()? {
                None => Ok((hashed(accounts::DEFAULT_PASSWORD)?, true)),
                Some(password) => {
                    let username = accounts::DEFAULT_USERNAME;
                    let key = config.admin_password_key();
                    accounts::policy(&password, username).map_err(|rule| (key, rule.text()))?;
                    Ok((hashed(&password)?, false))
                }
            }
        };
        match store::bootstrap(&postgres, initial) {
            Ok(_) => Ok(Self {
                keys,
                postgres,
                hashes: tokio::sync::Semaphore::new(HASHES_MAX),
            }),
            Err(store::Refused::Password(problem)) => Err(problem),
            Err(store::Refused::Postgres(reason)) => {
                Err(("postgres", Msg::AccountsBootstrap(&reason).text()))
            }
        }
    }
}

/// A username and a password checked against the accounts table.
enum Credentials {
    /// No hash permit was left: the 503 `busy`, nothing hashed.
    Busy,
    /// The table could not be read: Postgres's reason.
    Unreadable(String),
    /// The account read (suspended included), and why it is refused (`unknown`, `password`,
    /// `suspended`), none when the password is its own.
    Checked {
        account: Option<store::Account>,
        refusal: Option<&'static str>,
    },
}

/// The 503 `busy` of a hash beyond `HASHES_MAX`.
fn busy() -> Answer {
    let message = Msg::AccountsBusy.text();
    error(StatusCode::SERVICE_UNAVAILABLE, "busy", &message)
}

/// The one refusal of a sign-in or a password change: a 401 `invalid_credentials`.
fn invalid() -> Answer {
    let message = Msg::AccountsInvalid.text();
    error(StatusCode::UNAUTHORIZED, "invalid_credentials", &message)
}

/// The 503 of a session token that could not be signed.
fn unsigned() -> Answer {
    let message = Msg::AccountsUnsigned.text();
    error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message)
}

/// The stderr line of Postgres unreachable, with its `reason`.
fn postgres_line(reason: &str) {
    eprintln!(
        "ui: {}",
        Msg::PostgresUnreachable(reason).text().escape_debug()
    );
}

/// The stderr line of an `action` on the account `shown` refused for `reason`.
fn refused_line(subject: Option<&str>, action: &str, shown: &str, reason: &str) {
    let line = format!("refused ({reason})");
    write::audit(subject, None, action, shown, &line);
}

/// The 503 `audit_unavailable`, after its two stderr lines: Postgres's reason, and the action's.
fn unavailable(reason: &str, subject: Option<&str>, action: &str, arguments: &str) -> Answer {
    postgres_line(reason);
    let message = Msg::AuditUnavailable.text();
    write::audit(subject, None, action, arguments, &message);
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "audit_unavailable",
        &message,
    )
}

/// The 422 of a password the policy refuses: the rule's code and its message.
fn refused_by(rule: Rule) -> Answer {
    let document = json!({"error": {
        "code": "password_policy", "message": rule.text(), "rule": rule.code(),
    }});
    json_answer(StatusCode::UNPROCESSABLE_ENTITY, &document)
}

/// A body of `T`, or the fixed 400 (`message`) or 413 (`max`) refusing it: never serde's text,
/// which could quote a password.
fn parsed<T: for<'de> Deserialize<'de>>(
    body: &Body,
    max: usize,
    message: Msg,
) -> std::result::Result<T, Box<Answer>> {
    let bad = || {
        Box::new(error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            &message.text(),
        ))
    };
    match body {
        Ok(body) => serde_json::from_slice(body).map_err(|_| bad()),
        Err(StatusCode::PAYLOAD_TOO_LARGE) => {
            let message = Msg::UiTooLarge(max).text();
            Err(Box::new(error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "too_large",
                &message,
            )))
        }
        Err(_) => Err(bad()),
    }
}

/// `{token, expires_at, change_required}`: a session token and when it ends.
fn issued(token: String, expires: u64, change_required: bool) -> Answer {
    let document = json!({
        "token": token,
        "expires_at": crate::modules::rfc3339(expires),
        "change_required": change_required,
    });
    json_answer(StatusCode::OK, &document)
}

impl Console {
    /// Admission with local accounts (`local`) of a request to the API route `route` (`None`:
    /// a static file): the static files, `config` and `login` need no token; every other route a
    /// session token of this console, a token with the change-required mark only
    /// `account/password`. A refusal comes with the line the pre-authentication audit prints.
    pub(super) fn admit_local(
        self: &std::sync::Arc<Self>,
        local: &Local,
        headers: &HeaderMap,
        route: Option<&str>,
    ) -> std::result::Result<Session, (Box<Answer>, String)> {
        if matches!(route, None | Some("config") | Some(LOGIN)) {
            return Ok(Session::anonymous());
        }
        let found = crate::oidc::bearer(headers);
        let claims = match found {
            Ok(Some(token)) => (local.keys).verify(token, &self.ui.public_url, accounts::CONSOLE),
            _ => None,
        };
        let Some(claims) = claims else {
            let message = Msg::AccountsUnauthorized.text();
            let mut answer = error(StatusCode::UNAUTHORIZED, "unauthorized", &message);
            let challenge = match found {
                Ok(None) => "Bearer",
                _ => "Bearer error=\"invalid_token\"",
            };
            let challenge = header::HeaderValue::from_static(challenge);
            answer
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, challenge);
            return Err((Box::new(answer), message));
        };
        if claims.change_required && route != Some(PASSWORD) {
            let message = Msg::AccountsChangeRequired.text();
            let answer = error(StatusCode::FORBIDDEN, "password_change_required", &message);
            return Err((Box::new(answer), message));
        }
        let operator = claims.role == Role::Operator;
        let mut groups = claims.groups;
        groups.sort();
        groups.dedup();
        // With no groups table, every account reads every repository (ADR 0024).
        let granted = self.providers.grant(&groups, None);
        let grant = match self.providers.bounds() {
            true => granted.union(),
            false => crate::grant::Grant::All,
        };
        let teams = self.teams_on(route, operator, &groups, &grant);
        Ok(Session::local(claims.sub, operator, groups, teams, grant))
    }

    /// The account `username` names, checked for a sign-in or a password change with
    /// `password`: the account when one is read (suspended included), and why it is refused
    /// (`unknown`, `password`, `suspended`), none when `password` is its own. Exactly one hash
    /// either way: the account's, or `accounts::dummy` for no account, a hostile row (one line
    /// naming the table) and a suspended account, under a permit of `HASHES_MAX`.
    fn credentials(&self, local: &Local, username: &str, password: &str) -> Credentials {
        let found = match accounts::username_ok(username) {
            true => store::find(&local.postgres, username),
            false => Ok(Found::Unknown),
        };
        let found = match found {
            Ok(found) => found,
            Err(reason) => return Credentials::Unreadable(reason),
        };
        let Ok(_permit) = local.hashes.try_acquire() else {
            return Credentials::Busy;
        };
        let dummy = || accounts::verify(password, accounts::dummy());
        let (account, refusal) = match found {
            Found::Account(account) if account.suspended => {
                dummy();
                (Some(account), Some("suspended"))
            }
            Found::Account(account) => {
                let right = accounts::verify(password, &account.hash);
                (Some(account), (!right).then_some("password"))
            }
            Found::Bad => {
                let table = store::table(&local.postgres);
                eprintln!("ui: {}", Msg::AccountsBadRow(&table).text());
                dummy();
                (None, Some("unknown"))
            }
            Found::Unknown => {
                dummy();
                (None, Some("unknown"))
            }
        };
        Credentials::Checked { account, refusal }
    }

    /// `POST login`: `{username, password}` signed in, or the one 401 for an unknown username, a
    /// wrong password and a suspended account alike, each costing one hash; recorded
    /// `account.login` with the reason a refusal had, before the answer.
    pub(super) fn login(&self, local: &Local, body: &Body) -> Answer {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Login {
            username: String,
            password: String,
        }
        let login = match parsed::<Login>(body, LOGIN_MAX, Msg::AccountsLoginBody) {
            Ok(login) => login,
            Err(refused) => return *refused,
        };
        let (account, refusal) = match self.credentials(local, &login.username, &login.password) {
            Credentials::Busy => return busy(),
            Credentials::Unreadable(why) => return unavailable(&why, None, "account.login", "-"),
            Credentials::Checked { account, refusal } => (account, refusal),
        };
        let subject = (account.as_ref()).map(|a| accounts::subject(&a.username));
        // The account a stderr line names: never what an unknown username was, which could be a
        // password typed in the wrong field.
        let shown = account.as_ref().map_or("-", |a| a.username.as_str());
        let actor = Actor {
            via: "console",
            subject: subject.as_deref(),
            login: None,
            role: account.as_ref().map_or("none", |a| a.role.name()),
            owners: None,
        };
        let mut arguments = serde_json::Map::new();
        if let Some(account) = &account {
            arguments.insert("account".into(), json!(account.username));
        }
        let (answer, outcome, message) = match (&account, refusal) {
            (Some(account), None) => {
                let signed = local.keys.session(
                    &self.ui.public_url,
                    &account.username,
                    account.role,
                    &account.groups,
                    account.change_required,
                );
                let Ok((token, expires)) = signed else {
                    return unsigned();
                };
                let answer = issued(token, expires, account.change_required);
                (answer, Outcome::Accepted, Msg::AccountsSignedIn.text())
            }
            (_, refusal) => {
                let reason = refusal.unwrap_or("unknown");
                arguments.insert("reason".into(), json!(reason));
                (invalid(), Outcome::Refused, Msg::AccountsInvalid.text())
            }
        };
        let arguments = Value::Object(arguments);
        if let Err(reason) =
            (self.audit).record(&actor, "account.login", &arguments, outcome, &message)
        {
            return unavailable(&reason, subject.as_deref(), "account.login", shown);
        }
        match refusal {
            None => write::audit(subject.as_deref(), None, "account.login", shown, "accepted"),
            Some(reason) => refused_line(subject.as_deref(), "account.login", shown, reason),
        }
        answer
    }

    /// `POST account/password`: `{current, new}` for the session's own account. The new password
    /// passes the policy (a 422, not recorded, as every body check), the current one is checked
    /// (one 401 `invalid_credentials` otherwise, recorded), the new one differs from it (a 422),
    /// then the new hash is stored with no change required and a fresh session token without the
    /// mark answers; recorded `account.password` before anything changes.
    pub(super) fn password(&self, local: &Local, body: &Body, session: &Session) -> Answer {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Change {
            current: String,
            new: String,
        }
        let change = match parsed::<Change>(body, PASSWORD_BODY_MAX, Msg::AccountsPasswordBody) {
            Ok(change) => change,
            Err(refused) => return *refused,
        };
        let subject = session.subject.as_deref().unwrap_or_default();
        let username = accounts::username_of(subject).unwrap_or_default();
        if let Err(rule) = accounts::policy(&change.new, username) {
            return refused_by(rule);
        }
        let action = "account.password";
        let (account, reason) = match self.credentials(local, username, &change.current) {
            Credentials::Busy => return busy(),
            Credentials::Unreadable(why) => {
                return unavailable(&why, Some(subject), action, username);
            }
            Credentials::Checked { account, refusal } => (account, refusal),
        };
        let actor = write::actor(session, None);
        let mut arguments = json!({"account": username});
        if let Some(reason) = reason {
            arguments["reason"] = json!(reason);
            let message = Msg::AccountsInvalid.text();
            let recorded =
                (self.audit).record(&actor, action, &arguments, Outcome::Refused, &message);
            if let Err(why) = recorded {
                return unavailable(&why, Some(subject), action, username);
            }
            refused_line(Some(subject), action, username, reason);
            return invalid();
        }
        let account = account.expect("an account without a reason to refuse");
        // Checked once the current password is known to be right, so a wrong one is the 401.
        if change.new == change.current {
            return refused_by(Rule::Current);
        }
        let hashed = match local.hashes.try_acquire() {
            Ok(_permit) => accounts::hash(&change.new),
            Err(_) => return busy(),
        };
        let Ok(hashed) = hashed else {
            let message = Msg::AccountsUnavailable.text();
            return error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message);
        };
        let pending = match self.audit.pending(&actor, action, &arguments) {
            Ok(pending) => pending,
            Err(reason) => return unavailable(&reason, Some(subject), action, username),
        };
        let stored = store::set_password(&local.postgres, username, &hashed);
        let changed = Msg::AccountsPasswordChanged.text();
        match stored {
            Ok(true) => {}
            Ok(false) => {
                let message = Msg::AccountsInvalid.text();
                self.audit.finish(pending, Outcome::Refused, &message, None);
                refused_line(Some(subject), action, username, "unknown");
                return invalid();
            }
            Err(reason) => {
                let message = Msg::AccountsUnavailable.text();
                self.audit.finish(pending, Outcome::Failed, &message, None);
                postgres_line(&reason);
                write::audit(Some(subject), None, action, username, &message);
                return error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message);
            }
        }
        self.audit
            .finish(pending, Outcome::Accepted, &changed, None);
        write::audit(Some(subject), None, action, username, "accepted");
        let signed = local.keys.session(
            &self.ui.public_url,
            username,
            account.role,
            &account.groups,
            false,
        );
        match signed {
            Ok((token, expires)) => issued(token, expires, false),
            Err(_) => unsigned(),
        }
    }
}
