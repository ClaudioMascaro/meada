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
//!   an unknown username (hashed against `accounts::dummy`), a wrong password, a suspended and a
//!   locked account; `{token, expires_at, change_required}` on success, the stored hash redone
//!   when its parameters are weaker than the console's.
//! - `POST account/password` (`Console::password`): `{current, new}`, the policy applied, the
//!   current one checked (a wrong one counts toward the lockout), a fresh session token without
//!   the mark on success.
//! - Lockout (`store::attempt`): `accounts::LOCKOUT_FAILURES` consecutive wrong passwords, counted
//!   under the account row's lock, lock it for `accounts::LOCKOUT_MINUTES`; while locked, an
//!   attempt is refused without checking its password (the dummy hashed instead) and without
//!   extending the lock, recorded `locked` (`locked_now` for the failure that locked it).
//! - `meada ui reset-admin` (`reset_admin`): `admin` created or reactivated as an operator, its
//!   lock cleared, a password with a change required, recorded `account.reset_admin` `via` `cli`.
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
use crate::accounts::store::{self, Attempt, Refusal};
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
    /// The account read (locked and suspended included), and why it is refused, none when the
    /// password is its own.
    Checked {
        account: Option<store::Account>,
        refusal: Option<Refusal>,
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
    /// `password` (`store::attempt`, which counts a wrong one toward the lockout under the row's
    /// lock): the account when one is read, and why it is refused, none when `password` is its
    /// own. Exactly one hash either way, under a permit of `HASHES_MAX`: the account's, or
    /// `accounts::dummy` for no account, a hostile row (one line naming the table), a locked and a
    /// suspended account, so every refusal costs about what a wrong password does.
    fn credentials(&self, local: &Local, username: &str, password: &str) -> Credentials {
        let check = |phc: &str| {
            let _permit = local.hashes.try_acquire().ok()?;
            Some(accounts::verify(password, phc))
        };
        let attempt = match accounts::username_ok(username) {
            true => store::attempt(&local.postgres, username, check),
            false => Ok(Attempt::Unknown),
        };
        let (account, refusal) = match attempt {
            Err(reason) => return Credentials::Unreadable(reason),
            Ok(Attempt::Busy) => return Credentials::Busy,
            Ok(Attempt::Checked(account, refusal)) => {
                return Credentials::Checked {
                    account: Some(account),
                    refusal,
                };
            }
            Ok(Attempt::Unchecked(account, refusal)) => (Some(account), refusal),
            Ok(Attempt::Bad) => {
                let table = store::table(&local.postgres);
                eprintln!("ui: {}", Msg::AccountsBadRow(&table).text());
                (None, Refusal::Unknown)
            }
            Ok(Attempt::Unknown) => (None, Refusal::Unknown),
        };
        match check(accounts::dummy()) {
            None => Credentials::Busy,
            Some(_) => Credentials::Checked {
                account,
                refusal: Some(refusal),
            },
        }
    }

    /// After a sign-in that proved `password`: the account's stored hash redone under the
    /// console's own parameters when it is weaker (`accounts::weaker`), under a permit of
    /// `HASHES_MAX` (none left: the next sign-in does it). A failure leaves the old hash, which
    /// still admits, and one line.
    fn rehash(&self, local: &Local, account: &store::Account, password: &str) {
        if !accounts::weaker(&account.hash) {
            return;
        }
        let hashed = match local.hashes.try_acquire() {
            Ok(_permit) => accounts::hash(password),
            Err(_) => return,
        };
        let Ok(hashed) = hashed else {
            return;
        };
        match store::rehash(&local.postgres, &account.username, &account.hash, &hashed) {
            Ok(_) => {}
            Err(reason) => postgres_line(&reason),
        }
    }

    /// `POST login`: `{username, password}` signed in, or the one 401 for an unknown username, a
    /// wrong password, a suspended and a locked account alike, each costing one hash; recorded
    /// `account.login` with the reason a refusal had, before the answer. A success redoes a
    /// weaker stored hash (`rehash`).
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
        // A refusal's reason; with no account there is always one (`unknown`).
        let reason = refusal.unwrap_or(Refusal::Unknown).reason();
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
            (_, Some(_)) | (None, None) => {
                arguments.insert("reason".into(), json!(reason));
                (invalid(), Outcome::Refused, Msg::AccountsInvalid.text())
            }
        };
        let arguments = Value::Object(arguments);
        if let Err(why) =
            (self.audit).record(&actor, "account.login", &arguments, outcome, &message)
        {
            return unavailable(&why, subject.as_deref(), "account.login", shown);
        }
        match (refusal, &account) {
            (None, Some(account)) => {
                write::audit(subject.as_deref(), None, "account.login", shown, "accepted");
                self.rehash(local, account, &login.password);
            }
            _ => refused_line(subject.as_deref(), "account.login", shown, reason),
        }
        answer
    }

    /// `POST account/password`: `{current, new}` for the session's own account. The new password
    /// passes the policy (a 422, not recorded, as every body check), the current one is checked
    /// (one 401 `invalid_credentials` otherwise, recorded, a wrong one counting toward the
    /// lockout), the new one differs from it (a 422), then the new hash is stored with no change
    /// required and a fresh session token without the mark answers; recorded `account.password`
    /// before anything changes.
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
        if let Some(reason) = reason.map(Refusal::reason) {
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

/// `meada ui reset-admin --workspace <manifest>` (ADR 0024): the default account `admin` created,
/// or reactivated as an operator with its lock cleared, its password the value of
/// `admin_password_env` or `admin_password_file` when set (which must pass the policy), else one
/// generated now, with a change required either way. The record `account.reset_admin`, `via`
/// `cli`, is inserted before the account is written; one that cannot be written refuses with
/// nothing changed. The generated password, the one thing it prints on stdout; never on stderr
/// nor in the record.
pub fn reset_admin(manifest: &std::path::Path) -> crate::Result<Option<String>> {
    let read = crate::workspace::Manifest::read(manifest)?;
    let Some(config) = read.accounts else {
        let shown = manifest.display().to_string();
        return Err(Msg::AccountsResetNoSection(&shown).text().into());
    };
    let postgres =
        (config.postgres.clone()).expect("`Manifest::read` gives [accounts] its [postgres]");
    let problem = |(key, problem): crate::modules::Problem| format!("`{key}`: {problem}");
    // The password, and the key that names it (none: generated now).
    let (password, from) = match config.admin_password().map_err(problem)? {
        Some(password) => {
            let key = config.admin_password_key();
            (accounts::policy(&password, accounts::DEFAULT_USERNAME))
                .map_err(|rule| problem((key, rule.text())))?;
            (password, Some(key))
        }
        None => (accounts::generated(), None),
    };
    let hashed = accounts::hash(&password)?;
    let subject = crate::modules::cli_author();
    let arguments = json!({"account": accounts::DEFAULT_USERNAME});
    let created = crate::audit::cli(
        manifest,
        &subject,
        "account.reset_admin",
        Some(arguments),
        || store::reset_admin(&postgres, &hashed),
        |created| Msg::AccountsResetAdmin { created: *created }.text(),
        |_| false,
    )?;
    eprintln!("meada: {}", Msg::AccountsResetDone { created, from }.text());
    Ok(from.is_none().then_some(password))
}
