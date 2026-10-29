//! Console operations and their audit boundary. Refused uploads are never consumed.

use super::*;

#[cfg(feature = "refresh-lock")]
use crate::snapshot::versions::ApplyError;
#[cfg(feature = "refresh-lock")]
use meada_core::authorize::{Grant, Refusal, Role};

// ponytail: a pointer operation needs only an action and a generation id.
pub(super) const POINTER_MAX: usize = 1024;

// ponytail: fence recovery takes one optional unsigned decimal floor.
pub(super) const FENCE_MAX: usize = 1024;

// ponytail: applying a workspace takes only its version id.
pub(super) const APPLY_MAX: usize = 1024;

// ponytail: an editor save may carry a complete entries document and its message.
pub(super) const SAVE_MAX: usize = 4 << 20;

#[cfg(feature = "refresh-lock")]
fn decode<T: serde::de::DeserializeOwned>(
    body: &Body,
    max: usize,
) -> std::result::Result<T, (StatusCode, &'static str, String)> {
    let bad = || {
        (
            StatusCode::BAD_REQUEST,
            "bad_request",
            Msg::UiWriteBody.text(),
        )
    };
    let bytes = match body {
        Ok(bytes) => bytes,
        Err(StatusCode::PAYLOAD_TOO_LARGE) => {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "too_large",
                Msg::UiTooLarge(max).text(),
            ));
        }
        Err(_) => return Err(bad()),
    };
    // Never return serde's error text, which may echo a token or an entire request body.
    serde_json::from_slice(bytes).map_err(|_| bad())
}

fn refused(
    session: &Session,
    action: &str,
    arguments: &str,
    status: StatusCode,
    code: &str,
    message: &str,
) -> Answer {
    audit(
        session.subject.as_deref(),
        session.owners(),
        action,
        arguments,
        message,
    );
    error(status, code, message)
}

/// An accepted write answers its CLI line; `owners` are the groups a team operator's change
/// touched, for its audit line. An operational refusal is a 409 with the CLI message.
#[cfg(feature = "refresh-lock")]
fn result(
    session: &Session,
    action: &str,
    arguments: &str,
    result: std::result::Result<String, String>,
    owners: &[String],
) -> Answer {
    match result {
        Ok(result) => {
            let owners = session.team().then_some(owners);
            audit(
                session.subject.as_deref(),
                owners,
                action,
                arguments,
                &result,
            );
            json_answer(StatusCode::OK, &json!({"result": result}))
        }
        Err(message) => refused(
            session,
            action,
            arguments,
            StatusCode::CONFLICT,
            "conflict",
            &message,
        ),
    }
}

/// The role and grant a write is authorized as: the global operator's reads everything.
#[cfg(feature = "refresh-lock")]
fn acting(session: &Session) -> (Role, Grant) {
    match session.operator {
        true => (Role::Operator, Grant::All),
        false => (Role::TeamOperator, session.core_grant()),
    }
}

/// A refusal whose audit outcome is not its message.
#[cfg(feature = "refresh-lock")]
fn refusal(
    session: &Session,
    action: &str,
    outcome: &str,
    status: StatusCode,
    code: &str,
    message: &str,
) -> Answer {
    audit(
        session.subject.as_deref(),
        session.owners(),
        action,
        "-",
        outcome,
    );
    error(status, code, message)
}

/// A team operator that lost its role between admission and the lock gets a viewer's refusal.
#[cfg(feature = "refresh-lock")]
fn role_lost(session: &Session, action: &str, arguments: &str) -> Answer {
    let message = Msg::UiOperatorRequired.text();
    refused(
        session,
        action,
        arguments,
        StatusCode::FORBIDDEN,
        "forbidden",
        &message,
    )
}

/// The 403 `entries_refused` (ADR 0015): each refused entry with its change and reason, and
/// `hidden`, the count of those whose identity `hiding` does not read (an apply names no
/// identity outside the grant; a save refuses only what the subject submitted or reads).
/// Nothing was written. The audit outcome is the message, the named identities and the count.
#[cfg(feature = "refresh-lock")]
fn entries_refused(
    session: &Session,
    action: &str,
    arguments: &str,
    hiding: Option<&Grant>,
    refused: Vec<Refusal>,
) -> Answer {
    let message = Msg::WorkspaceRefused(refused.len()).text();
    let (named, hidden): (Vec<_>, Vec<_>) =
        (refused.iter()).partition(|r| hiding.is_none_or(|grant| grant.reads(&r.identity)));
    let names: Vec<&str> = named.iter().map(|r| r.identity.as_str()).collect();
    let outcome = format!("{message} ({}; hidden {})", names.join(", "), hidden.len());
    audit(
        session.subject.as_deref(),
        session.owners(),
        action,
        arguments,
        &outcome,
    );
    let body = json!({"error": {
        "code": "entries_refused",
        "message": message,
        "refused": named,
        "hidden": hidden.len(),
    }});
    json_answer(StatusCode::FORBIDDEN, &body)
}

impl Console {
    /// The session was authorized before the listener read its bounded body.
    pub(super) fn write(
        &self,
        route: &str,
        method: &Method,
        body: &Body,
        session: &Session,
    ) -> Answer {
        let action = operation(route).expect("operation checked by route");
        if *method != Method::POST {
            return refused(
                session,
                action,
                "-",
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method Not Allowed",
            );
        }
        #[cfg(feature = "refresh-lock")]
        if route == "pointer" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Pointer {
                action: String,
                generation: String,
            }
            let pointer = match decode::<Pointer>(body, POINTER_MAX) {
                Ok(pointer)
                    if matches!(pointer.action.as_str(), "set" | "retain" | "drop")
                        && meada_core::ids::is_id(&pointer.generation) =>
                {
                    pointer
                }
                Ok(_) => {
                    return refused(
                        session,
                        action,
                        "-",
                        StatusCode::BAD_REQUEST,
                        "bad_request",
                        &Msg::UiWriteBody.text(),
                    );
                }
                Err((status, code, message)) => {
                    return refused(session, action, "-", status, code, &message);
                }
            };
            let author = session.subject.as_deref().expect("write subject admitted");
            let run = match pointer.action.as_str() {
                "set" => crate::snapshot::operation::pointer_set,
                "retain" => crate::snapshot::operation::pointer_retain,
                _ => crate::snapshot::operation::pointer_drop,
            };
            let result_line =
                run(&self.file, &pointer.generation, author).map_err(|e| e.to_string());
            let action = format!("pointer {}", pointer.action);
            return result(session, &action, &pointer.generation, result_line, &[]);
        }
        #[cfg(feature = "refresh-lock")]
        if route == "fence/recover" {
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum Floor {
                Unsigned(u64),
                Decimal(String),
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Recover {
                at_least: Option<Floor>,
            }
            let recover = match decode::<Recover>(body, FENCE_MAX) {
                Ok(recover) => recover,
                Err((status, code, message)) => {
                    return refused(session, action, "-", status, code, &message);
                }
            };
            let floor = match recover.at_least {
                None => None,
                Some(Floor::Unsigned(number)) => Some(number.to_string()),
                Some(Floor::Decimal(text)) => match meada_core::ids::fence(&text) {
                    Some(number) => Some(number.to_string()),
                    None => {
                        return refused(
                            session,
                            action,
                            "-",
                            StatusCode::BAD_REQUEST,
                            "bad_request",
                            &Msg::UiWriteBody.text(),
                        );
                    }
                },
            };
            let author = session.subject.as_deref().expect("write subject admitted");
            let done =
                crate::snapshot::operation::fence_recover(&self.file, floor.as_deref(), author)
                    .map(|(fence, _)| fence.to_string())
                    .map_err(|e| e.to_string());
            return result(session, action, floor.as_deref().unwrap_or("-"), done, &[]);
        }
        #[cfg(feature = "refresh-lock")]
        if route == "workspace/versions" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Save {
                entries: String,
                message: String,
            }
            let save = match decode::<Save>(body, SAVE_MAX) {
                Ok(save) => save,
                Err((status, code, message)) => {
                    return refused(session, action, "-", status, code, &message);
                }
            };
            let author = session.subject.as_deref().expect("write subject admitted");
            let (role, grant) = acting(session);
            return match crate::snapshot::versions::save(
                &self.file,
                &save.entries,
                &save.message,
                author,
                role,
                &session.groups,
                &grant,
            ) {
                Ok((saved, owners)) => {
                    result(session, action, &saved.id, Ok(saved.id.clone()), &owners)
                }
                Err(crate::snapshot::versions::SaveError::Invalid(problems)) => refusal(
                    session,
                    action,
                    &Msg::UiEntriesInvalid.text(),
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_entries",
                    &problems.join("\n"),
                ),
                Err(crate::snapshot::versions::SaveError::Unauthorized(refused)) => {
                    entries_refused(session, action, "-", None, refused)
                }
                Err(crate::snapshot::versions::SaveError::RoleLost) => {
                    role_lost(session, action, "-")
                }
                Err(crate::snapshot::versions::SaveError::Refused(message)) => refused(
                    session,
                    action,
                    "-",
                    StatusCode::CONFLICT,
                    "conflict",
                    &message,
                ),
            };
        }
        #[cfg(feature = "refresh-lock")]
        if route == "workspace/apply" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Apply {
                version: String,
            }
            let apply = match decode::<Apply>(body, APPLY_MAX) {
                Ok(apply) if meada_core::ids::is_id(&apply.version) => apply,
                Ok(_) => {
                    return refused(
                        session,
                        action,
                        "-",
                        StatusCode::BAD_REQUEST,
                        "bad_request",
                        &Msg::UiWriteBody.text(),
                    );
                }
                Err((status, code, message)) => {
                    return refused(session, action, "-", status, code, &message);
                }
            };
            let author = session.subject.as_deref().expect("write subject admitted");
            let (role, grant) = acting(session);
            let done = crate::snapshot::versions::apply(
                &self.file,
                &apply.version,
                author,
                role,
                &session.groups,
                &grant,
            );
            return match done {
                Ok(applied) => result(
                    session,
                    action,
                    &apply.version,
                    Ok(applied.line),
                    &applied.owners,
                ),
                Err(ApplyError::Refused(refused)) => {
                    entries_refused(session, action, &apply.version, Some(&grant), refused)
                }
                Err(ApplyError::RoleLost) => role_lost(session, action, &apply.version),
                Err(ApplyError::Failed(message)) => {
                    result(session, action, &apply.version, Err(message), &[])
                }
            };
        }
        let _ = body;
        refused(
            session,
            action,
            "-",
            StatusCode::CONFLICT,
            "conflict",
            &Msg::NeedsFeature {
                section: "refresh_lock",
                feature: "refresh-lock",
            }
            .text(),
        )
    }
}

/// The four operation routes; only their fixed names can enter a pre-authentication audit.
pub(super) fn operation(route: &str) -> Option<&'static str> {
    Some(match route {
        "pointer" => "pointer",
        "fence/recover" => "fence recover",
        "workspace/versions" => "workspace save",
        "workspace/apply" => "workspace apply",
        _ => return None,
    })
}

/// One line even for hostile subjects, arguments and multiline catalog messages. Neither
/// tokens nor raw request bodies enter this function; pre-authentication subjects are absent.
pub(super) fn audit(
    subject: Option<&str>,
    owners: Option<&[String]>,
    action: &str,
    arguments: &str,
    outcome: &str,
) {
    // A team operator's line names the owner groups after the subject (ADR 0015, #324); the
    // JSON array escapes its own controls.
    let as_owners = owners.map_or(String::new(), |owners| format!(" as {}", json!(owners)));
    eprintln!(
        "ui: {}{as_owners} {} {}: {}",
        subject.unwrap_or("-").escape_debug(),
        action.escape_debug(),
        arguments.escape_debug(),
        outcome.escape_debug(),
    );
}
