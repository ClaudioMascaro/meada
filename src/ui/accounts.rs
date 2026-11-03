//! The console's side of local accounts (ADR 0024, feature `accounts`): with `[accounts]` and no
//! `[oidc]`, the console signs its own session tokens and admits them as bearers.
//!
//! Break glass (`[accounts] break_glass = true` beside `[oidc]`, #563): only the default account
//! signs in (any other username is the same 401, recorded `not_break_glass`, with no table read),
//! and its session token is admitted beside the identity provider's tokens
//! (`Console::admit_break_glass`): a bearer whose issuer is the console's `public_url` is checked
//! against the signing key alone, any other bearer by the identity provider alone. The session is
//! the global operator's; it may change its own password, and no other route of local accounts
//! exists.
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
//! - `POST account/tokens` (`Console::agent_token`, #560): `{surface, hours}` (`nodes`, or
//!   `proposals` beside `[proposals]`, #562; 1 to `accounts::AGENT_HOURS_MAX` hours,
//!   `accounts::AGENT_HOURS` when absent) answers `{token, expires_at, surface}` once: an agent
//!   token for that surface's audience signed with the account's role, groups and login as its
//!   row stands now, a random token id, recorded `account.token` (surface, hours, token id, never
//!   the token) before it is returned. A suspended or unknown account gets the role refusal, a
//!   403 `forbidden`, recorded; a change-required token never reaches it (admission's 403). The
//!   console admits no agent token: its audience is not the console's.
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
use crate::accounts::{self, Holder, Keys, Role, Rule};
use crate::audit::{Actor, Outcome};
use crate::msg::Msg;

mod administration;
mod github;
pub(super) use administration::{ADMIN_BODY_MAX, is_administration};
pub(super) use github::{GITHUB, GITHUB_BODY_MAX, GITHUB_REMOVE};

// ponytail: the most bytes of a sign-in's body: a username and a password of at most 128
// characters, each escaped, with room to spare; past it a 413.
pub(super) const LOGIN_MAX: usize = 4 << 10;
// ponytail: the most bytes of a password change's body, two passwords; past it a 413.
pub(super) const PASSWORD_BODY_MAX: usize = 4 << 10;
// ponytail: the most bytes of an agent token's request, a surface and a number; past it a 413.
pub(super) const TOKENS_BODY_MAX: usize = 1 << 10;
// ponytail: the most password hashes the console computes at once, each 19 MiB of memory; a
// further sign-in or password change is a 503 `busy` without hashing.
pub(super) const HASHES_MAX: usize = 4;

/// The fixed start of the line that shows a generated first password: English in every locale,
/// as `listening on` is, so a deployer's `grep` finds it.
pub(super) const FIRST_PASSWORD: &str = "accounts: default account admin created; first password: ";

/// The routes of this module.
pub(super) const LOGIN: &str = "login";
pub(super) const PASSWORD: &str = "account/password";
pub(super) const TOKENS: &str = "account/tokens";

/// What the console holds for local accounts.
pub(super) struct Local {
    keys: Keys,
    postgres: crate::postgres::Config,
    hashes: tokio::sync::Semaphore,
    /// Beside `[oidc]`: the default account alone, as the global operator (#563).
    pub(super) break_glass: bool,
    /// The default account's first password when this start generated it, until `announce`
    /// prints it.
    first: std::sync::Mutex<Option<String>>,
    /// With `[accounts.github]` (#561): its `api`, the one endpoint the console calls, and the
    /// agent it calls it with.
    github: Option<github::Verifier>,
}

impl Local {
    /// The console's local accounts of `config`: the key pair read and checked, then the default
    /// account created when the table is empty, its initial password generated with a change
    /// required (`announce` shows it once), or the one the deployer set (which must pass the
    /// policy) without. Each problem is a key and its message.
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
                None => {
                    let password = accounts::generated();
                    Ok((hashed(&password)?, Some(password)))
                }
                Some(password) => {
                    let username = accounts::DEFAULT_USERNAME;
                    let key = config.admin_password_key();
                    accounts::policy(&password, username).map_err(|rule| (key, rule.text()))?;
                    Ok((hashed(&password)?, None))
                }
            }
        };
        match store::bootstrap(&postgres, initial) {
            Ok(first) => Ok(Self {
                keys,
                postgres,
                hashes: tokio::sync::Semaphore::new(HASHES_MAX),
                break_glass: config.break_glass,
                first: std::sync::Mutex::new(first),
                github: config.api.clone().map(github::Verifier::new),
            }),
            Err(store::Refused::Password(problem)) => Err(problem),
            Err(store::Refused::Postgres(reason)) => {
                Err(("postgres", Msg::AccountsBootstrap(&reason).text()))
            }
        }
    }

    /// The one stderr line of the first password this start generated (ADR 0024, amended
    /// 2026-11-02), printed once and never through the audit: right after the listening line, or
    /// on drop when the console stops before it (a later start check or the bind failing), since
    /// the account exists either way.
    pub(super) fn announce(&self) {
        let first = self.first.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(password) = first {
            eprintln!("{FIRST_PASSWORD}{password}");
        }
    }

    /// Whom a session token of `account` is for: its role, groups and verified login, or under
    /// break glass the global operator with no group and no login, which is what admission makes
    /// of it anyway.
    fn signed_as<'a>(&self, account: &'a store::Account) -> Holder<'a> {
        let username = &account.username;
        match self.break_glass {
            true => Holder {
                username,
                role: Role::Operator,
                groups: &[],
                login: None,
            },
            false => Holder {
                username,
                role: account.role,
                groups: &account.groups,
                login: account.login.as_deref(),
            },
        }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.announce();
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

/// The 401 of a request without a valid session token, with `WWW-Authenticate: Bearer`
/// (`error="invalid_token"` when one was `sent`), and the line the pre-authentication audit
/// prints.
fn unauthorized(sent: bool) -> (Box<Answer>, String) {
    let message = Msg::AccountsUnauthorized.text();
    let mut answer = error(StatusCode::UNAUTHORIZED, "unauthorized", &message);
    let challenge = accounts::challenge(sent);
    let challenge = header::HeaderValue::from_static(challenge);
    answer
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, challenge);
    (Box::new(answer), message)
}

/// The 403 `password_change_required` of a session token with the mark, on any route but the
/// password change, and its line.
fn change_required() -> (Box<Answer>, String) {
    let message = Msg::AccountsChangeRequired.text();
    let answer = error(StatusCode::FORBIDDEN, "password_change_required", &message);
    (Box::new(answer), message)
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

/// The 503 `unavailable` of the accounts table out of reach, after Postgres's line.
fn unreachable(reason: &str) -> Answer {
    postgres_line(reason);
    let message = Msg::AccountsUnavailable.text();
    error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message)
}

/// The line of a row of the accounts table that fails its checks, naming the table alone.
fn bad_row(local: &Local) {
    let table = store::table(&local.postgres);
    eprintln!("ui: {}", Msg::AccountsBadRow(&table).text());
}

/// `password` hashed under a permit of `HASHES_MAX`: the 503 `busy` with none left, the 503
/// `unavailable` when it cannot be hashed.
fn hashed(local: &Local, password: &str) -> std::result::Result<String, Box<Answer>> {
    let hashed = match local.hashes.try_acquire() {
        Ok(_permit) => accounts::hash(password),
        Err(_) => return Err(Box::new(busy())),
    };
    hashed.map_err(|_| {
        let message = Msg::AccountsUnavailable.text();
        Box::new(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            &message,
        ))
    })
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
    /// `GET admission` with `[accounts]` (#558): `accounts`, the mode (`accounts`, or
    /// `break_glass` beside `[oidc]`) and the public key's fingerprint, set into `document`, the
    /// identity provider's under break glass. With local accounts alone, the document is the
    /// groups table (`[accounts.groups]`, each group's identities `session` reads, `*` kept, null
    /// without it) and the GitHub grant provider's state as an identity provider's shows it.
    pub(super) fn accounts_admission(
        &self,
        local: &Local,
        document: Option<Value>,
        session: &Session,
    ) -> Value {
        let mode = match local.break_glass {
            true => "break_glass",
            false => "accounts",
        };
        let accounts = json!({"mode": mode, "public_key": local.keys.fingerprint()});
        if let Some(mut document) = document {
            document["accounts"] = accounts;
            return document;
        }
        let table = self
            .bootstrap
            .accounts
            .as_ref()
            .and_then(|a| a.groups.as_ref());
        let groups = table.map(|table| crate::grant::bounded(table, &|i| session.reads(i)));
        let mut document = json!({"accounts": accounts, "groups": groups});
        if let Some(github) = self.providers.admission() {
            document["github"] = github;
        }
        document
    }

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
            return Err(unauthorized(!matches!(found, Ok(None))));
        };
        if claims.change_required && route != Some(PASSWORD) {
            return Err(change_required());
        }
        let operator = claims.role == Role::Operator;
        Ok(self.local_session(claims.sub, operator, claims.groups, claims.login, route))
    }

    /// The session of the local account `subject` on `route`: global operator when `operator`,
    /// its `groups` sorted, the grant the grant providers compute from them and its verified
    /// `login` (#561; every repository with no table, ADR 0024), the login, the GitHub grant's
    /// state, and a team operator's teams.
    fn local_session(
        self: &std::sync::Arc<Self>,
        subject: String,
        operator: bool,
        groups: Vec<String>,
        login: Option<String>,
        route: Option<&str>,
    ) -> Session {
        let granted = self.providers.local(&groups, login.as_deref());
        let admitted = crate::grant::Admitted::new(Some(subject), operator, groups, granted);
        let teams = self.teams_on(route, operator, &admitted.groups, &admitted.grant);
        Session::local(admitted, teams)
    }

    /// Before a console write of a local account (`write`: an operation, a named workspace's
    /// write, account administration; ADR 0024, #557): the session as the account's row stands
    /// now, so a suspension or a role change takes effect on writes at once, while reads keep the
    /// token's until it expires. An active account's role, groups and verified login come from
    /// its row (under
    /// break glass the session stands as admitted, the global operator's); a suspended, unknown
    /// or hostile one (one line naming the table) writes as a viewer with no group that operates
    /// no named workspace (`Session::suspended`), and so gets the role refusal its write already
    /// has. `login` and the password change are no such
    /// write: they read the row themselves. Postgres unreachable is a 503 `unavailable`, with its
    /// line, and the write's message.
    pub(super) fn standing(
        self: &std::sync::Arc<Self>,
        session: Session,
        route: Option<&str>,
        write: bool,
    ) -> std::result::Result<Session, (Box<Answer>, String)> {
        let Some(local) = &self.accounts else {
            return Ok(session);
        };
        if !write || !session.account {
            return Ok(session);
        }
        let subject = session.subject.clone().unwrap_or_default();
        let username = accounts::username_of(&subject).unwrap_or_default();
        let (operator, groups, login) = match store::standing(&local.postgres, username) {
            // Under break glass the default account is the global operator whatever its row's
            // role (`Local::signed_as`), with every repository: only its suspension takes a write.
            Ok(store::Standing::Active(..)) if local.break_glass => return Ok(session),
            Ok(store::Standing::Active(role, groups, login)) => {
                let session =
                    self.local_session(subject, role == Role::Operator, groups, login, route);
                return Ok(session);
            }
            Ok(store::Standing::Suspended | store::Standing::Unknown) => (false, Vec::new(), None),
            Ok(store::Standing::Bad) => {
                bad_row(local);
                (false, Vec::new(), None)
            }
            Err(reason) => {
                let answer = unreachable(&reason);
                return Err((Box::new(answer), Msg::AccountsUnavailable.text()));
            }
        };
        // The subject stays for the record of the refusal, but operates no named workspace.
        let mut session = self.local_session(subject, operator, groups, login, route);
        session.suspended = true;
        Ok(session)
    }

    /// Admission under break glass (`[oidc]` beside, #563) of a request to the API route `route`
    /// (`None`: a static file), or `None` when the identity provider's admission decides. The
    /// static files, `config` and `login` need no token. A bearer whose issuer is this console's
    /// `public_url` (`accounts::issued_by`, before any key is tried) is checked against the signing
    /// key alone and must be the default account's session token, else the 401; a token with the
    /// change-required mark opens `account/password` alone. Every other request, a bearer of any
    /// other issuer included, is the identity provider's to admit, so it never sees this
    /// console's tokens nor the signing key the identity provider's. The session is the global
    /// operator's, with no group, reading every repository.
    pub(super) fn admit_break_glass(
        &self,
        local: &Local,
        headers: &HeaderMap,
        route: Option<&str>,
    ) -> Option<std::result::Result<Session, (Box<Answer>, String)>> {
        if matches!(route, None | Some("config") | Some(LOGIN)) {
            return Some(Ok(Session::anonymous()));
        }
        let Ok(Some(token)) = crate::oidc::bearer(headers) else {
            return None;
        };
        let issuer = &self.ui.public_url;
        if !accounts::issued_by(token, issuer) {
            return None;
        }
        let claims = (local.keys.verify(token, issuer, accounts::CONSOLE))
            .filter(|claims| claims.username() == accounts::DEFAULT_USERNAME);
        Some(match claims {
            None => Err(unauthorized(true)),
            Some(claims) if claims.change_required && route != Some(PASSWORD) => {
                Err(change_required())
            }
            Some(claims) => {
                let admitted = crate::grant::Admitted {
                    subject: Some(claims.sub),
                    operator: true,
                    groups: Vec::new(),
                    login: None,
                    grant: crate::grant::Grant::All,
                    github: None,
                };
                Ok(Session::local(admitted, Vec::new()))
            }
        })
    }

    /// Under break glass, a sign-in naming any account but the default one: refused
    /// `not_break_glass` without reading the table, after the one hash every refusal costs (the
    /// dummy), under a permit of `HASHES_MAX`.
    fn not_break_glass(&self, local: &Local, password: &str) -> Credentials {
        let Ok(_permit) = local.hashes.try_acquire() else {
            return Credentials::Busy;
        };
        let _ = accounts::verify(password, accounts::dummy());
        Credentials::Checked {
            account: None,
            refusal: Some(Refusal::NotBreakGlass),
        }
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
                bad_row(local);
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
    /// wrong password, a suspended and a locked account alike (and under break glass any account
    /// but the default one), each costing one hash; recorded `account.login` with the reason a
    /// refusal had, before the answer. A success redoes a weaker stored hash (`rehash`).
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
        let checked = match local.break_glass && login.username != accounts::DEFAULT_USERNAME {
            true => self.not_break_glass(local, &login.password),
            false => self.credentials(local, &login.username, &login.password),
        };
        let (account, refusal) = match checked {
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
                let holder = local.signed_as(account);
                let signed =
                    (local.keys).session(&self.ui.public_url, &holder, account.change_required);
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
        let hashed = match hashed(local, &change.new) {
            Ok(hashed) => hashed,
            Err(refused) => return *refused,
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
        let holder = local.signed_as(&account);
        let signed = (local.keys).session(&self.ui.public_url, &holder, false);
        match signed {
            Ok((token, expires)) => issued(token, expires, false),
            Err(_) => unsigned(),
        }
    }
}

impl Console {
    /// `POST account/tokens` (#560): an agent token for the session's own account, for the
    /// surface `nodes`, or `proposals` when the bootstrap has `[proposals]` (#562), valid `hours`
    /// (`accounts::AGENT_HOURS` when absent). The body's checks (400, 413, 422) are not
    /// recorded, as for every body check. The account's row is read now,
    /// as before every console write: suspended, unknown or hostile (one line naming the table) is
    /// the role refusal, recorded; active, the token carries the row's role and groups. The record
    /// `account.token` (the surface, the hours and the token id, never the token) is written
    /// before the token is returned; one that cannot be written gives no token.
    pub(super) fn agent_token(&self, local: &Local, body: &Body, session: &Session) -> Answer {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Asked {
            surface: String,
            hours: Option<u64>,
        }
        let asked = match parsed::<Asked>(
            body,
            TOKENS_BODY_MAX,
            Msg::AccountsTokenBody(accounts::AGENT_HOURS_MAX),
        ) {
            Ok(asked) => asked,
            Err(refused) => return *refused,
        };
        let unprocessable =
            |code, message: String| error(StatusCode::UNPROCESSABLE_ENTITY, code, &message);
        // The proposal server's surface only beside its section (#562).
        #[cfg(feature = "proposals")]
        let proposals = self.bootstrap.proposals.is_some();
        #[cfg(not(feature = "proposals"))]
        let proposals = false;
        let (surface, audience) = match asked.surface.as_str() {
            accounts::NODES_SURFACE => (accounts::NODES_SURFACE, accounts::NODES),
            accounts::PROPOSALS_SURFACE if proposals => {
                (accounts::PROPOSALS_SURFACE, accounts::PROPOSALS)
            }
            _ => {
                let message = Msg::AccountsTokenSurface(proposals).text();
                return unprocessable("invalid_surface", message);
            }
        };
        let hours = asked.hours.unwrap_or(accounts::AGENT_HOURS);
        if !(1..=accounts::AGENT_HOURS_MAX).contains(&hours) {
            let message = Msg::AccountsTokenHours(accounts::AGENT_HOURS_MAX).text();
            return unprocessable("invalid_hours", message);
        }
        let subject = session.subject.as_deref().unwrap_or_default();
        let username = accounts::username_of(subject).unwrap_or_default();
        let action = "account.token";
        let actor = write::actor(session, None);
        let mut arguments =
            json!({"account": username, "surface": surface, "hours": hours.to_string()});
        let (role, groups, login) = match store::standing(&local.postgres, username) {
            Ok(store::Standing::Active(role, groups, login)) => (role, groups, login),
            Ok(standing) => {
                if matches!(standing, store::Standing::Bad) {
                    bad_row(local);
                }
                let message = Msg::AccountsTokenRefused.text();
                write::audit(Some(subject), None, action, username, &message);
                self.audit.refused(&actor, action, &arguments, &message);
                return error(StatusCode::FORBIDDEN, "forbidden", &message);
            }
            Err(reason) => return unreachable(&reason),
        };
        let issuer = &self.ui.public_url;
        let holder = Holder {
            username,
            role,
            groups: &groups,
            login: login.as_deref(),
        };
        let signed = (local.keys).agent(issuer, &holder, audience, hours);
        let Ok((token, expires, id)) = signed else {
            return unsigned();
        };
        arguments["token_id"] = json!(id);
        let message = Msg::AccountsTokenIssued.text();
        if let Err(why) =
            (self.audit).record(&actor, action, &arguments, Outcome::Accepted, &message)
        {
            return unavailable(&why, Some(subject), action, username);
        }
        write::audit(Some(subject), None, action, username, "accepted");
        let document = json!({
            "token": token,
            "expires_at": crate::modules::rfc3339(expires),
            "surface": surface,
        });
        json_answer(StatusCode::OK, &document)
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
