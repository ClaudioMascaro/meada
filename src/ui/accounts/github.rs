use hyper::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Answer, Body, Console, Local, Session, error, json_answer, parsed, write};
use super::{bad_row, unavailable, unreachable, unsigned};
use crate::accounts::store::{self, Change, Changed, Standing};
use crate::accounts::{self, Holder};
use crate::audit::Outcome;
use crate::msg::Msg;

pub(in crate::ui) const GITHUB: &str = "account/github";
pub(in crate::ui) const GITHUB_REMOVE: &str = "account/github/remove";

// ponytail: the most bytes of a login verification's body, one personal access token (GitHub's
// are under 300 characters); past it a 413.
pub(in crate::ui) const GITHUB_BODY_MAX: usize = 1 << 10;
// ponytail: how long the one call to GitHub may take, from connection to last byte; past it the
// 503 `github_unavailable`.
const GITHUB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
// ponytail: the most bytes of GitHub's answer to `GET /user` (a few KiB); past it the 503
// `github_unavailable`.
const GITHUB_ANSWER_MAX: u64 = 64 << 10;

pub(super) struct Verifier {
    api: String,
    agent: ureq::Agent,
}

#[derive(Debug, PartialEq)]
enum Unverified {
    Refused,
    Invalid,
    Unavailable(String),
}

impl Verifier {
    pub(super) fn new(api: String) -> Self {
        Self::within(api, GITHUB_TIMEOUT)
    }

    fn within(api: String, timeout: std::time::Duration) -> Self {
        Self {
            api,
            agent: crate::modules::agent(timeout),
        }
    }

    fn login(&self, token: &str) -> Result<String, Unverified> {
        #[derive(Deserialize)]
        struct User {
            login: String,
        }
        let url = format!("{}/user", self.api);
        let scrubbed = |e: ureq::Error| {
            let reason = format!("{url}: {e}").replace(token, "***");
            Unverified::Unavailable(reason)
        };
        let sent = (self.agent.get(&url))
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "meada")
            .call();
        let mut response = sent.map_err(scrubbed)?;
        let status = response.status().as_u16();
        if (400..500).contains(&status) && status != 429 {
            return Err(Unverified::Refused);
        }
        if !response.status().is_success() {
            return Err(Unverified::Unavailable(format!(
                "{url}: http status {status}"
            )));
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(GITHUB_ANSWER_MAX + 1);
        let body = body.read_to_vec().map_err(scrubbed)?;
        let user: User = serde_json::from_slice(&body)
            .map_err(|_| Unverified::Unavailable(format!("{url}: no login in the answer")))?;
        let login = user.login.to_ascii_lowercase();
        match crate::grant::login_ok(&login) {
            true => Ok(login),
            false => Err(Unverified::Invalid),
        }
    }
}

fn token_ok(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| b.is_ascii_graphic())
}

fn answered(local: &Local, issuer: &str, holder: &Holder) -> Answer {
    match local.keys.session(issuer, holder, false) {
        Ok((token, expires)) => {
            let document = json!({
                "login": holder.login,
                "token": token,
                "expires_at": crate::modules::rfc3339(expires),
                "change_required": false,
            });
            json_answer(StatusCode::OK, &document)
        }
        Err(_) => unsigned(),
    }
}

impl Console {
    fn github_standing(
        &self,
        local: &Local,
        session: &Session,
        action: &str,
        arguments: &Value,
    ) -> std::result::Result<(), Box<Answer>> {
        let subject = session.subject.as_deref().unwrap_or_default();
        let username = accounts::username_of(subject).unwrap_or_default();
        match store::standing(&local.postgres, username) {
            Ok(Standing::Active(..)) => Ok(()),
            Ok(standing) => {
                if standing == Standing::Bad {
                    bad_row(local);
                }
                let message = Msg::AccountsGithubForbidden.text();
                write::audit(Some(subject), None, action, username, &message);
                let actor = write::actor(session, None);
                self.audit.refused(&actor, action, arguments, &message);
                Err(Box::new(error(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    &message,
                )))
            }
            Err(reason) => Err(Box::new(unreachable(&reason))),
        }
    }

    pub(in crate::ui) fn verify_github(
        &self,
        local: &Local,
        body: &Body,
        session: &Session,
    ) -> Answer {
        let Some(verifier) = &local.github else {
            return error(StatusCode::NOT_FOUND, "not_found", &Msg::UiNoRoute.text());
        };
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Pasted {
            token: String,
        }
        let pasted = match parsed::<Pasted>(body, GITHUB_BODY_MAX, Msg::AccountsGithubBody) {
            Ok(pasted) => pasted,
            Err(refused) => return *refused,
        };
        if !token_ok(&pasted.token) {
            let message = Msg::AccountsGithubToken.text();
            return error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_token", &message);
        }
        let subject = session.subject.as_deref().unwrap_or_default();
        let username = accounts::username_of(subject).unwrap_or_default();
        let action = "account.github";
        let mut arguments = json!({"account": username});
        if let Err(refused) = self.github_standing(local, session, action, &arguments) {
            return *refused;
        }
        let actor = write::actor(session, None);
        let login = match verifier.login(&pasted.token) {
            Ok(login) => login,
            Err(unverified) => {
                let (status, code, reason, message) = match &unverified {
                    Unverified::Refused => (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "github_refused",
                        "github_refused",
                        Msg::AccountsGithubRefused.text(),
                    ),
                    Unverified::Invalid => (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "invalid_login",
                        "invalid_login",
                        Msg::AccountsGithubLogin.text(),
                    ),
                    Unverified::Unavailable(why) => {
                        eprintln!("ui: {}", Msg::AccountsGithubUnreachable(why).text());
                        (
                            StatusCode::SERVICE_UNAVAILABLE,
                            "github_unavailable",
                            "github_unavailable",
                            Msg::AccountsGithubUnavailable.text(),
                        )
                    }
                };
                arguments["reason"] = json!(reason);
                super::refused_line(Some(subject), action, username, code);
                let outcome = Outcome::of(status != StatusCode::SERVICE_UNAVAILABLE);
                let recorded = (self.audit).record(&actor, action, &arguments, outcome, &message);
                if let Err(why) = recorded {
                    return unavailable(&why, Some(subject), action, username);
                }
                return error(status, code, &message);
            }
        };
        arguments["login"] = json!(login);
        let change = Change::Github(login);
        self.github_change(local, session, action, &arguments, &change, username)
    }

    pub(in crate::ui) fn remove_own_github(&self, local: &Local, session: &Session) -> Answer {
        if local.github.is_none() {
            return error(StatusCode::NOT_FOUND, "not_found", &Msg::UiNoRoute.text());
        }
        let subject = session.subject.as_deref().unwrap_or_default();
        let username = accounts::username_of(subject).unwrap_or_default();
        let action = "account.remove_github";
        let arguments = json!({"account": username});
        if let Err(refused) = self.github_standing(local, session, action, &arguments) {
            return *refused;
        }
        let change = Change::RemoveGithub;
        self.github_change(local, session, action, &arguments, &change, username)
    }

    fn github_change(
        &self,
        local: &Local,
        session: &Session,
        action: &str,
        arguments: &Value,
        change: &Change,
        username: &str,
    ) -> Answer {
        let subject = session.subject.as_deref();
        let actor = write::actor(session, None);
        let pending = match self.audit.pending(&actor, action, arguments) {
            Ok(pending) => pending,
            Err(reason) => return unavailable(&reason, subject, action, username),
        };
        let (status, code, message) = match store::change(&local.postgres, username, change) {
            Ok(Changed::Done(listed)) => {
                let message = match change {
                    Change::RemoveGithub => Msg::AccountsGithubRemoved.text(),
                    _ => Msg::AccountsGithubVerified.text(),
                };
                self.audit
                    .finish(pending, Outcome::Accepted, &message, None);
                write::audit(subject, None, action, username, "accepted");
                return match listed {
                    Some(listed) if !listed.suspended => {
                        let holder = Holder {
                            username,
                            role: listed.role,
                            groups: &listed.groups,
                            login: listed.github_login.as_deref(),
                        };
                        answered(local, &self.ui.public_url, &holder)
                    }
                    Some(_) => {
                        let message = Msg::AccountsGithubForbidden.text();
                        error(StatusCode::FORBIDDEN, "forbidden", &message)
                    }
                    None => {
                        bad_row(local);
                        let message = Msg::AccountsUnknown.text();
                        error(StatusCode::NOT_FOUND, "not_found", &message)
                    }
                };
            }
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
            Ok(changed) => {
                if changed == Changed::Bad {
                    bad_row(local);
                }
                let message = Msg::AccountsUnknown.text();
                (StatusCode::NOT_FOUND, "not_found", message)
            }
            Err(reason) => {
                let answer = unreachable(&reason);
                let message = Msg::AccountsUnavailable.text();
                self.audit.finish(pending, Outcome::Failed, &message, None);
                write::audit(subject, None, action, username, &message);
                return answer;
            }
        };
        self.audit.finish(pending, Outcome::Refused, &message, None);
        super::refused_line(subject, action, username, code);
        error(status, code, &message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_token_is_visible_ascii() {
        for ok in ["ghp_abc123", "github_pat_11AAAA_x-y.z", "!"] {
            assert!(token_ok(ok), "{ok}");
        }
        for refused in ["", "ghp abc", "ghp\nabc", "ghp\u{e9}", "ghp\tabc"] {
            assert!(!token_ok(refused), "{refused:?}");
        }
    }

    #[test]
    fn the_login_is_read_from_get_user_once_and_the_token_goes_nowhere_else() {
        let token = "ghp_secret0123456789";
        let big = format!(
            "{{\"login\": \"{}\"}}",
            "a".repeat(GITHUB_ANSWER_MAX as usize)
        );
        for (status, body, expected) in [
            (
                "200 OK",
                r#"{"login": "Ana-1", "id": 7}"#,
                Ok("ana-1".to_string()),
            ),
            (
                "401 Unauthorized",
                r#"{"message": "Bad credentials"}"#,
                Err("refused"),
            ),
            ("403 Forbidden", "{}", Err("refused")),
            ("429 Too Many Requests", "{}", Err("unavailable")),
            ("502 Bad Gateway", "oops", Err("unavailable")),
            ("200 OK", r#"{"name": "Ana"}"#, Err("unavailable")),
            ("200 OK", big.as_str(), Err("unavailable")),
            ("200 OK", r#"{"login": "ana smith"}"#, Err("invalid")),
        ] {
            let (api, asked) = once(status, body);
            let found = Verifier::new(api).login(token);
            let request = asked.join().unwrap();
            assert!(request.starts_with("GET /user HTTP/1.1\r\n"), "{request}");
            let bearer = format!("authorization: bearer {token}\r\n");
            assert!(request.to_ascii_lowercase().contains(&bearer), "{request}");
            match (found, expected) {
                (Ok(login), Ok(expected)) => assert_eq!(login, expected),
                (Err(Unverified::Refused), Err("refused")) => {}
                (Err(Unverified::Invalid), Err("invalid")) => {}
                (Err(Unverified::Unavailable(why)), Err("unavailable")) => {
                    assert!(!why.contains(token) && !why.contains("oops"), "{why}");
                }
                (found, expected) => panic!("{status} {body}: {found:?}, not {expected:?}"),
            }
        }
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        match Verifier::new(api).login(token) {
            Err(Unverified::Unavailable(why)) => assert!(!why.contains(token), "{why}"),
            found => panic!("{found:?}"),
        }
    }

    #[test]
    fn a_silent_github_is_unavailable_at_the_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let held = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(2));
            drop(stream);
        });
        let started = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(200);
        let found = Verifier::within(api, timeout).login("ghp_secret0123456789");
        assert!(
            matches!(found, Err(Unverified::Unavailable(_))),
            "{found:?}"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        held.join().unwrap();
    }

    fn once(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let answer = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
             connection: close\r\n\r\n{body}",
            body.len()
        );
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            while !request.ends_with(b"\r\n\r\n") {
                let n = stream.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
            }
            let _ = stream.write_all(answer.as_bytes());
            String::from_utf8_lossy(&request).into_owned()
        });
        (api, served)
    }
}
