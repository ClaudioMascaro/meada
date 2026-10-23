//! Console operations and their audit boundary. Refused uploads are never consumed.

use super::*;

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
    audit(session.subject.as_deref(), action, arguments, message);
    error(status, code, message)
}

#[cfg(feature = "refresh-lock")]
fn result(
    session: &Session,
    action: &str,
    arguments: &str,
    result: std::result::Result<String, String>,
) -> Answer {
    match result {
        Ok(result) => {
            audit(session.subject.as_deref(), action, arguments, &result);
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
            return result(session, &action, &pointer.generation, result_line);
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
            return result(session, action, floor.as_deref().unwrap_or("-"), done);
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
            return match crate::snapshot::versions::save(
                &self.file,
                &save.entries,
                &save.message,
                author,
            ) {
                Ok(saved) => result(session, action, &saved.id, Ok(saved.id.clone())),
                Err(crate::snapshot::versions::SaveError::Invalid(problems)) => {
                    audit(
                        session.subject.as_deref(),
                        action,
                        "-",
                        &Msg::UiEntriesInvalid.text(),
                    );
                    error(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "invalid_entries",
                        &problems.join("\n"),
                    )
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
            let done = crate::snapshot::versions::apply(&self.file, &apply.version, author)
                .map_err(|e| e.to_string());
            return result(session, action, &apply.version, done);
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
pub(super) fn audit(subject: Option<&str>, action: &str, arguments: &str, outcome: &str) {
    eprintln!(
        "ui: {} {} {}: {}",
        subject.unwrap_or("-").escape_debug(),
        action.escape_debug(),
        arguments.escape_debug(),
        outcome.escape_debug(),
    );
}
