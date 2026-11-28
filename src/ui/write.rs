use super::*;
use crate::audit::{Actor, Audit, Outcome, Pending};

#[cfg(feature = "refresh-lock")]
use crate::snapshot::versions::{Acting, ApplyError};
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
    serde_json::from_slice(bytes).map_err(|_| bad())
}

pub(super) type PendingRecord<'a> = Option<(&'a Audit, Pending)>;

pub(super) fn finish(
    record: PendingRecord,
    outcome: Outcome,
    message: &str,
    owners: Option<&[String]>,
) {
    if let Some((audit, pending)) = record {
        audit.finish(pending, outcome, message, owners);
    }
}

pub(super) fn actor<'a>(session: &'a Session, owners: Option<&'a [String]>) -> Actor<'a> {
    Actor {
        via: "console",
        subject: session.subject.as_deref(),
        login: session.login.as_deref(),
        role: session.role(),
        owners,
    }
}

pub(super) fn refused(
    record: PendingRecord,
    session: &Session,
    action: &str,
    arguments: &str,
    status: StatusCode,
    code: &str,
    message: &str,
) -> Answer {
    unsuccessful(
        record,
        Outcome::Refused,
        session,
        action,
        arguments,
        status,
        code,
        message,
    )
}

#[expect(clippy::too_many_arguments)]
fn unsuccessful(
    record: PendingRecord,
    outcome: Outcome,
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
    finish(record, outcome, message, session.owners());
    error(status, code, message)
}

#[cfg(feature = "refresh-lock")]
fn result(
    record: PendingRecord,
    session: &Session,
    action: &str,
    arguments: &str,
    result: std::result::Result<String, (String, Outcome)>,
    owners: &[String],
) -> Answer {
    match result {
        Ok(result) => {
            let owners = session.is_team_operator().then_some(owners);
            audit(
                session.subject.as_deref(),
                owners,
                action,
                arguments,
                &result,
            );
            finish(record, Outcome::Accepted, &result, owners);
            json_answer(StatusCode::OK, &json!({"result": result}))
        }
        Err((message, outcome)) => unsuccessful(
            record,
            outcome,
            session,
            action,
            arguments,
            StatusCode::CONFLICT,
            "conflict",
            &message,
        ),
    }
}

#[cfg(feature = "refresh-lock")]
fn outcome(e: Box<dyn std::error::Error>) -> (String, Outcome) {
    let outcome = Outcome::of(crate::snapshot::generations::OpError::refused(&*e));
    (e.to_string(), outcome)
}

#[cfg(feature = "refresh-lock")]
fn grant(session: &Session) -> Grant {
    match session.operator {
        true => Grant::All,
        false => session.core_grant(),
    }
}

#[cfg(feature = "refresh-lock")]
fn acting<'a>(session: &'a Session, grant: &'a Grant) -> Acting<'a> {
    Acting {
        role: match session.operator {
            true => Role::Operator,
            false => Role::TeamOperator,
        },
        groups: &session.groups,
        grant,
        login: session.login.as_deref(),
        github_host: session.github_host(),
    }
}

#[cfg(feature = "refresh-lock")]
fn refused_with_outcome(
    record: PendingRecord,
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
    finish(record, Outcome::Refused, outcome, session.owners());
    error(status, code, message)
}

#[cfg(all(debug_assertions, feature = "refresh-lock"))]
fn pause_before_lock() {
    let Some(path) = std::env::var_os("MEADA_PAUSE_BEFORE_LOCK") else {
        return;
    };
    let beside = |end: &str| {
        let mut name = path.clone();
        name.push(end);
        std::path::PathBuf::from(name)
    };
    if std::fs::remove_file(&path).is_err() {
        return;
    }
    let _ = std::fs::write(beside(".reached"), "");
    for _ in 0..300 {
        if beside(".go").exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    eprintln!(
        "ui: MEADA_PAUSE_BEFORE_LOCK: no {}.go after 30 s, going on",
        path.to_string_lossy()
    );
}

#[cfg(feature = "refresh-lock")]
fn role_lost(record: PendingRecord, session: &Session, action: &str, arguments: &str) -> Answer {
    let message = Msg::UiOperatorRequired.text();
    refused(
        record,
        session,
        action,
        arguments,
        StatusCode::FORBIDDEN,
        "forbidden",
        &message,
    )
}

#[cfg(feature = "refresh-lock")]
fn entries_refused(
    record: PendingRecord,
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
    finish(record, Outcome::Refused, &outcome, session.owners());
    let body = json!({"error": {
        "code": "entries_refused",
        "message": message,
        "refused": named,
        "hidden": hidden.len(),
    }});
    json_answer(StatusCode::FORBIDDEN, &body)
}

impl Console {
    #[cfg(feature = "refresh-lock")]
    pub(super) fn pending(
        &self,
        session: &Session,
        (line_action, line_arguments): (&str, &str),
        action: &str,
        arguments: Value,
    ) -> std::result::Result<PendingRecord<'_>, Box<Answer>> {
        let actor = actor(session, session.owners());
        match self.audit.pending(&actor, action, &arguments) {
            Ok(pending) => Ok(Some((&self.audit, pending))),
            Err(reason) => {
                eprintln!(
                    "ui: {}",
                    Msg::PostgresUnreachable(&reason).text().escape_debug()
                );
                Err(Box::new(refused(
                    None,
                    session,
                    line_action,
                    line_arguments,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "audit_unavailable",
                    &Msg::AuditUnavailable.text(),
                )))
            }
        }
    }

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
                None,
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
                        None,
                        session,
                        action,
                        "-",
                        StatusCode::BAD_REQUEST,
                        "bad_request",
                        &Msg::UiWriteBody.text(),
                    );
                }
                Err((status, code, message)) => {
                    return refused(None, session, action, "-", status, code, &message);
                }
            };
            let action = format!("pointer {}", pointer.action);
            let generation = pointer.generation.as_str();
            let record = match self.pending(
                session,
                (&action, generation),
                &format!("pointer.{}", pointer.action),
                json!({"generation": generation}),
            ) {
                Ok(record) => record,
                Err(answer) => return *answer,
            };
            let author = session.subject.as_deref().expect("write subject admitted");
            let run = match pointer.action.as_str() {
                "set" => crate::snapshot::operation::pointer_set,
                "retain" => crate::snapshot::operation::pointer_retain,
                _ => crate::snapshot::operation::pointer_drop,
            };
            let result_line = run(&self.file, generation, author).map_err(outcome);
            return result(record, session, &action, generation, result_line, &[]);
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
                    return refused(None, session, action, "-", status, code, &message);
                }
            };
            let floor = match recover.at_least {
                None => None,
                Some(Floor::Unsigned(number)) => Some(number.to_string()),
                Some(Floor::Decimal(text)) => match meada_core::ids::fence(&text) {
                    Some(number) => Some(number.to_string()),
                    None => {
                        return refused(
                            None,
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
            let shown = floor.as_deref().unwrap_or("-");
            let arguments = match &floor {
                Some(floor) => json!({"at_least": floor}),
                None => json!({}),
            };
            let record = match self.pending(session, (action, shown), "fence.recover", arguments) {
                Ok(record) => record,
                Err(answer) => return *answer,
            };
            let author = session.subject.as_deref().expect("write subject admitted");
            let done =
                crate::snapshot::operation::fence_recover(&self.file, floor.as_deref(), author)
                    .map(|(fence, _)| fence.to_string())
                    .map_err(outcome);
            return result(record, session, action, shown, done, &[]);
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
                    return refused(None, session, action, "-", status, code, &message);
                }
            };
            let author = session.subject.as_deref().expect("write subject admitted");
            let invalid = |record, problems: Vec<String>| {
                refused_with_outcome(
                    record,
                    session,
                    action,
                    &Msg::UiEntriesInvalid.text(),
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_entries",
                    &problems.join("\n"),
                )
            };
            let id =
                match crate::snapshot::versions::version_id(&save.entries, &save.message, author) {
                    Ok(id) => id,
                    Err(problems) => return invalid(None, problems),
                };
            let arguments = json!({"version": id, "message": save.message});
            let record = match self.pending(session, (action, "-"), "workspace.save", arguments) {
                Ok(record) => record,
                Err(answer) => return *answer,
            };
            let grant = grant(session);
            #[cfg(all(debug_assertions, feature = "refresh-lock"))]
            pause_before_lock();
            return match crate::snapshot::versions::save(
                &self.file,
                &save.entries,
                &save.message,
                author,
                &acting(session, &grant),
            ) {
                Ok((saved, owners)) => result(
                    record,
                    session,
                    action,
                    &saved.id,
                    Ok(saved.id.clone()),
                    &owners,
                ),
                Err(crate::snapshot::versions::SaveError::Invalid(problems)) => {
                    invalid(record, problems)
                }
                Err(crate::snapshot::versions::SaveError::Unauthorized(refused)) => {
                    entries_refused(record, session, action, "-", None, refused)
                }
                Err(crate::snapshot::versions::SaveError::RoleLost) => {
                    role_lost(record, session, action, "-")
                }
                Err(crate::snapshot::versions::SaveError::Refused(message)) => {
                    let done = Err((message, Outcome::Refused));
                    result(record, session, action, "-", done, &[])
                }
                Err(crate::snapshot::versions::SaveError::Failed(message)) => {
                    let done = Err((message, Outcome::Failed));
                    result(record, session, action, "-", done, &[])
                }
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
                        None,
                        session,
                        action,
                        "-",
                        StatusCode::BAD_REQUEST,
                        "bad_request",
                        &Msg::UiWriteBody.text(),
                    );
                }
                Err((status, code, message)) => {
                    return refused(None, session, action, "-", status, code, &message);
                }
            };
            let version = apply.version.as_str();
            let arguments = json!({"version": version});
            let record =
                match self.pending(session, (action, version), "workspace.apply", arguments) {
                    Ok(record) => record,
                    Err(answer) => return *answer,
                };
            let author = session.subject.as_deref().expect("write subject admitted");
            let grant = grant(session);
            #[cfg(all(debug_assertions, feature = "refresh-lock"))]
            pause_before_lock();
            let done = crate::snapshot::versions::apply(
                &self.file,
                version,
                author,
                &acting(session, &grant),
            );
            return match done {
                Ok(applied) => result(
                    record,
                    session,
                    action,
                    version,
                    Ok(applied.line),
                    &applied.owners,
                ),
                Err(ApplyError::Refused(refused)) => {
                    entries_refused(record, session, action, version, Some(&grant), refused)
                }
                Err(ApplyError::RoleLost) => role_lost(record, session, action, version),
                Err(e @ (ApplyError::Conflict(_) | ApplyError::Failed(_))) => {
                    let outcome = Outcome::of(e.refused());
                    let done = Err((e.to_string(), outcome));
                    result(record, session, action, version, done, &[])
                }
            };
        }
        let _ = body;
        let message = Msg::NeedsFeature {
            section: "refresh_lock",
            feature: "refresh-lock",
        }
        .text();
        refused(
            None,
            session,
            action,
            "-",
            StatusCode::CONFLICT,
            "conflict",
            &message,
        )
    }
}

const OPERATIONS: [(&str, &str, &str); 4] = [
    ("pointer", "pointer", "pointer"),
    ("fence/recover", "fence recover", "fence.recover"),
    ("workspace/versions", "workspace save", "workspace.save"),
    ("workspace/apply", "workspace apply", "workspace.apply"),
];

pub(super) fn recorded(route: &str) -> Option<&'static str> {
    (OPERATIONS.iter())
        .find(|(r, ..)| *r == route)
        .map(|(.., action)| *action)
}

pub(super) fn operation(route: &str) -> Option<&'static str> {
    (OPERATIONS.iter())
        .find(|(r, ..)| *r == route)
        .map(|(_, name, _)| *name)
}

pub(super) fn audit(
    subject: Option<&str>,
    owners: Option<&[String]>,
    action: &str,
    arguments: &str,
    outcome: &str,
) {
    let as_owners = owners.map_or(String::new(), |owners| format!(" as {}", json!(owners)));
    eprintln!(
        "ui: {}{as_owners} {} {}: {}",
        subject.unwrap_or("-").escape_debug(),
        action.escape_debug(),
        arguments.escape_debug(),
        outcome.escape_debug(),
    );
}
