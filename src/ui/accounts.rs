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

pub(super) const FIRST_PASSWORD: &str = "accounts: default account admin created; first password: ";

pub(super) const LOGIN: &str = "login";
pub(super) const PASSWORD: &str = "account/password";
pub(super) const TOKENS: &str = "account/tokens";

pub(super) struct Local {
    keys: Keys,
    postgres: crate::postgres::Config,
    hashes: tokio::sync::Semaphore,
    pub(super) break_glass: bool,
    first: std::sync::Mutex<Option<String>>,
    github: Option<github::Verifier>,
}

impl Local {
    pub(super) fn start(
        config: &accounts::Config,
    ) -> std::result::Result<Self, crate::modules::Problem> {
        let keys = config.keys()?;
        let postgres =
            (config.postgres.clone()).expect("`Manifest::read` gives [accounts] its [postgres]");
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

    pub(super) fn announce(&self) {
        let first = self.first.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(password) = first {
            eprintln!("{FIRST_PASSWORD}{password}");
        }
    }

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

enum Credentials {
    Busy,
    Unreadable(String),
    Checked {
        account: Option<store::Account>,
        refusal: Option<Refusal>,
    },
}

fn busy() -> Answer {
    let message = Msg::AccountsBusy.text();
    error(StatusCode::SERVICE_UNAVAILABLE, "busy", &message)
}

fn invalid() -> Answer {
    let message = Msg::AccountsInvalid.text();
    error(StatusCode::UNAUTHORIZED, "invalid_credentials", &message)
}

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

fn change_required() -> (Box<Answer>, String) {
    let message = Msg::AccountsChangeRequired.text();
    let answer = error(StatusCode::FORBIDDEN, "password_change_required", &message);
    (Box::new(answer), message)
}

fn unsigned() -> Answer {
    let message = Msg::AccountsUnsigned.text();
    error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message)
}

fn postgres_line(reason: &str) {
    eprintln!(
        "ui: {}",
        Msg::PostgresUnreachable(reason).text().escape_debug()
    );
}

fn unreachable(reason: &str) -> Answer {
    postgres_line(reason);
    let message = Msg::AccountsUnavailable.text();
    error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message)
}

fn bad_row(local: &Local) {
    let table = store::table(&local.postgres);
    eprintln!("ui: {}", Msg::AccountsBadRow(&table).text());
}

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

fn refused_line(subject: Option<&str>, action: &str, shown: &str, reason: &str) {
    let line = format!("refused ({reason})");
    write::audit(subject, None, action, shown, &line);
}

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

fn refused_by(rule: Rule) -> Answer {
    let document = json!({"error": {
        "code": "password_policy", "message": rule.text(), "rule": rule.code(),
    }});
    json_answer(StatusCode::UNPROCESSABLE_ENTITY, &document)
}

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

fn issued(token: String, expires: u64, change_required: bool) -> Answer {
    let document = json!({
        "token": token,
        "expires_at": crate::modules::rfc3339(expires),
        "change_required": change_required,
    });
    json_answer(StatusCode::OK, &document)
}

impl Console {
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
        let mut session = self.local_session(subject, operator, groups, login, route);
        session.suspended = true;
        Ok(session)
    }

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
