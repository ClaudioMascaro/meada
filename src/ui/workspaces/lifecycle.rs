//! The version lifecycle of named workspaces in the console (#343, ADR 0021 contract 4): save,
//! apply and rollback, rename, delete and the operator list, and the reads of a workspace's
//! versions and history. Identity, authority and grant come from the token and the permissions
//! set, never from the body. A write is checked in the order the contract fixes: the body, then
//! the discovery rule (a workspace the caller may not see is the one 404 of an unknown id), the
//! caller's authority, the base, the entries; the atomic boundary (`lifecycle::transact`) decides
//! all of them again under the workspace's row lock, so a stale page, a revoked grant or a
//! forged payload authorizes nothing.

use std::collections::BTreeSet;

use meada_core::authorize::Grant;

use super::*;
use crate::workspaces::lifecycle::{self as ops, Error, Op, Target};

// ponytail: the request bodies of the lifecycle routes (ADR 0021): a save carries a manifest of
// at most 64 KiB and a message, the others a few short fields.
const APPLY_BODY_MAX: usize = 1 << 10;
const RENAME_BODY_MAX: usize = 4 << 10;
const DELETE_BODY_MAX: usize = 1 << 10;
const OPERATORS_BODY_MAX: usize = 16 << 10;

/// The cap of the body of the lifecycle route at `path` (`/api/v1/workspaces/<id>/<op>`), none
/// for any other path: the listener reads no more than this.
pub(in crate::ui) fn body_max(path: &str) -> Option<usize> {
    let rest = path.strip_prefix("/api/v1/workspaces/")?;
    let (id, op) = rest.split_once('/')?;
    meada_core::ids::is_id(id).then_some(())?;
    match op {
        "versions" => Some(super::CREATE_MAX),
        "apply" => Some(APPLY_BODY_MAX),
        "rename" => Some(RENAME_BODY_MAX),
        "delete" => Some(DELETE_BODY_MAX),
        "operators" => Some(OPERATORS_BODY_MAX),
        _ => None,
    }
}

/// The operations by route, with their audit action (an apply that goes back to a version that
/// was current before is recorded as `workspaces.rollback`).
const ROUTES: [(&str, &str); 5] = [
    ("versions", "workspaces.save"),
    ("apply", "workspaces.apply"),
    ("rename", "workspaces.rename"),
    ("delete", "workspaces.delete"),
    ("operators", "workspaces.operators"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    entries: String,
    message: String,
    base: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Apply {
    version: String,
    base: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rename {
    name: String,
    description: Option<String>,
    base: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delete {
    base: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Operators {
    #[serde(default)]
    add: Vec<NewOperator>,
    #[serde(default)]
    remove: Vec<String>,
    base: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewOperator {
    subject: String,
    login: String,
}

/// What a lifecycle write has resolved before its transaction: the audit action and arguments,
/// the operation's inputs and the entries it stores.
struct Plan {
    action: &'static str,
    version: Option<String>,
    /// For a save: the document to store before the rows that name it.
    document: Option<Vec<Value>>,
    identities: Vec<String>,
    changed: Vec<String>,
    message: String,
    name: String,
    description: String,
    add: Vec<(String, String)>,
    remove: Vec<String>,
}

impl Plan {
    fn new(action: &'static str) -> Self {
        Plan {
            action,
            version: None,
            document: None,
            identities: Vec::new(),
            changed: Vec::new(),
            message: String::new(),
            name: String::new(),
            description: String::new(),
            add: Vec::new(),
            remove: Vec::new(),
        }
    }
}

impl Console {
    /// A refusal found before the pending record: logged, recorded `refused` when Postgres
    /// answers, and answered.
    fn turned_away(
        &self,
        session: &Session,
        action: &str,
        arguments: &Value,
        id: &str,
        (status, code, message): (StatusCode, &str, String),
    ) -> Answer {
        super::super::write::audit(session.subject.as_deref(), None, action, id, &message);
        let actor = super::super::write::actor(session, None);
        self.audit.refused(&actor, action, arguments, &message);
        error(status, code, &message)
    }

    /// `POST workspaces/<id>/<op>`: the lifecycle writes. `None` when `rest` is no such route.
    pub(super) fn operate(&self, rest: &str, body: &Body, session: &Session) -> Option<Answer> {
        let (id, op) = rest.split_once('/')?;
        let (_, action) = ROUTES.iter().find(|(route, _)| *route == op)?;
        self.bootstrap.workspaces.as_ref()?;
        let bad = |status, code: &str, message: &str| {
            refused(None, session, action, id, status, code, message)
        };
        let bytes = match body {
            Ok(bytes) => bytes,
            Err(StatusCode::PAYLOAD_TOO_LARGE) => {
                let max = body_max(&format!("/api/v1/workspaces/{rest}")).unwrap_or(0);
                let message = Msg::UiTooLarge(max).text();
                return Some(bad(StatusCode::PAYLOAD_TOO_LARGE, "too_large", &message));
            }
            Err(_) => {
                let shape = Self::shape(op);
                let message = Msg::WorkspacesOpBody(shape).text();
                return Some(bad(StatusCode::BAD_REQUEST, "bad_request", &message));
            }
        };
        let body_bad = || {
            let message = Msg::WorkspacesOpBody(Self::shape(op)).text();
            bad(StatusCode::BAD_REQUEST, "bad_request", &message)
        };
        // Never serde's text, which may echo the body.
        let plan = match op {
            "versions" => serde_json::from_slice::<Save>(bytes)
                .map_err(|_| ())
                .map(|b| (b.base, Decoded::Save(b))),
            "apply" => serde_json::from_slice::<Apply>(bytes)
                .map_err(|_| ())
                .map(|b| (b.base, Decoded::Apply(b))),
            "rename" => serde_json::from_slice::<Rename>(bytes)
                .map_err(|_| ())
                .map(|b| (b.base, Decoded::Rename(b))),
            "delete" => serde_json::from_slice::<Delete>(bytes)
                .map_err(|_| ())
                .map(|b| (b.base, Decoded::Delete)),
            _ => serde_json::from_slice::<Operators>(bytes)
                .map_err(|_| ())
                .map(|b| (b.base, Decoded::Operators(b))),
        };
        let Ok((base, request)) = plan else {
            return Some(body_bad());
        };
        if base < 1 || !meada_core::ids::is_id(id) {
            return Some(match meada_core::ids::is_id(id) {
                true => body_bad(),
                false => Self::no_workspace(),
            });
        }
        Some(self.write_one(id, action, base, request, session))
    }

    /// The shape of the body of the route `op`, for the 400 that names it.
    fn shape(op: &str) -> &'static str {
        match op {
            "versions" => "{\"entries\", \"message\", \"base\"}",
            "apply" => "{\"version\", \"base\"}",
            "rename" => "{\"name\", \"description\"?, \"base\"}",
            "delete" => "{\"base\"}",
            _ => "{\"add\"?: [{\"subject\", \"login\"}], \"remove\"?: [subject], \"base\"}",
        }
    }

    fn write_one(
        &self,
        id: &str,
        action: &'static str,
        base: i64,
        request: Decoded,
        session: &Session,
    ) -> Answer {
        let (Some(section), Some(policy)) = (&self.bootstrap.workspaces, self.workspace_policy())
        else {
            return Self::no_workspace();
        };
        let subject = match session.subject.as_deref() {
            Some(subject) if !subject.is_empty() => subject,
            _ => {
                let message = Msg::UiOperatorRequired.text();
                return refused(
                    None,
                    session,
                    action,
                    id,
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    &message,
                );
            }
        };
        let administrator = session.operator;
        let authority = match administrator {
            true => "platform_administrator",
            false => "workspace_operator",
        };
        let grant = Self::read_grant(session);
        // What a refusal may name: the platform administrator reads every entry of a management
        // view, anyone else what its grant reads.
        let naming = match administrator {
            true => Grant::All,
            false => grant.clone(),
        };
        let unprocessable = |code: &str, message: String| {
            refused(
                None,
                session,
                action,
                id,
                StatusCode::UNPROCESSABLE_ENTITY,
                code,
                &message,
            )
        };
        // The discovery rule: a workspace the caller neither operates nor reads whole does not
        // exist for it, so the answer is an unknown id's, whatever the body.
        let record = match self.visible_records(session, Some(id)) {
            Ok(mut records) => match records.pop() {
                Some(record) => record,
                None => {
                    let arguments = json!({"workspace": id, "base": base.to_string()});
                    let message = Msg::WorkspacesNotFound.text();
                    let answer = (StatusCode::NOT_FOUND, "not_found", message);
                    return self.turned_away(session, action, &arguments, id, answer);
                }
            },
            Err(answer) => return *answer,
        };
        let arguments = |version: Option<&str>| {
            let mut arguments = json!({
                "workspace": id, "base": base.to_string(), "authority": authority,
            });
            if let Some(version) = version {
                arguments["version"] = version.into();
            }
            arguments
        };
        if !Self::operates(session, &record) {
            let message = Msg::WorkspacesNotOperator.text();
            let answer = (StatusCode::FORBIDDEN, "not_operator", message);
            return self.turned_away(session, action, &arguments(None), id, answer);
        }
        if matches!(request, Decoded::Operators(_)) && !administrator {
            let message = Msg::WorkspacesNotAdministrator.text();
            let answer = (StatusCode::FORBIDDEN, "not_administrator", message);
            return self.turned_away(session, action, &arguments(None), id, answer);
        }
        if record.seq != base {
            let message = Msg::WorkspacesStale(record.seq).text();
            let answer = (StatusCode::CONFLICT, "stale_base", message);
            return self.turned_away(session, action, &arguments(None), id, answer);
        }
        // Resolve the operation against the stored current version.
        let mut plan = Plan::new(action);
        let current = if matches!(request, Decoded::Save(_) | Decoded::Apply(_)) {
            match self.version_entries(&record) {
                Ok(current) => current,
                Err(why) => return self.unreadable(session, action, id, &why),
            }
        } else {
            Vec::new()
        };
        match &request {
            Decoded::Save(save) => {
                if let Some(problem) = named::description_problem(&save.message).or_else(|| {
                    (save.message.is_empty())
                        .then(|| Msg::WorkspacesOpBody(Self::shape("versions")).text())
                }) {
                    return unprocessable("invalid_workspace", problem);
                }
                let submitted =
                    match named::version_entries("entries", &save.entries, section.max_entries) {
                        Ok(entries) => entries,
                        Err(problems) => {
                            return unprocessable("invalid_entries", problems.join("\n"));
                        }
                    };
                // The platform administrator submits the whole manifest; an operator submits the
                // entries it reads and the version stored keeps the others in place.
                let stored = match administrator {
                    true => submitted,
                    false => named::compose(&grant, &current, &submitted),
                };
                if stored.is_empty() {
                    return unprocessable("invalid_entries", Msg::WorkspacesEmptyVersion.text());
                }
                if stored.len() > section.max_entries {
                    let message = meada_core::msg::Msg::NamedTooMany(section.max_entries).text();
                    return unprocessable("invalid_entries", message);
                }
                plan.version = Some(named::version_id(&stored));
                plan.changed = named::changed(&current, &stored);
                plan.identities = named::identities(&stored);
                plan.message = save.message.clone();
                plan.document = Some(stored);
            }
            Decoded::Apply(apply) => {
                if !meada_core::ids::is_id(&apply.version) {
                    let message = Msg::WorkspacesOpBody(Self::shape("apply")).text();
                    return refused(
                        None,
                        session,
                        action,
                        id,
                        StatusCode::BAD_REQUEST,
                        "bad_request",
                        &message,
                    );
                }
                let row =
                    crate::modules::block_on(ops::version(policy.postgres, id, &apply.version));
                match row.map_err(|e| e.to_string()).and_then(|r| r) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        let message = Msg::WorkspacesUnknownVersion.text();
                        let answer = (StatusCode::NOT_FOUND, "unknown_version", message);
                        let arguments = arguments(Some(&apply.version));
                        return self.turned_away(session, action, &arguments, id, answer);
                    }
                    Err(why) => return self.unreadable(session, action, id, &why),
                }
                let target = match self.document(&apply.version) {
                    Ok(target) => target,
                    Err(why) => return self.unreadable(session, action, id, &why),
                };
                plan.changed = named::changed(&current, &target);
                plan.identities = named::identities(&target);
                plan.version = Some(apply.version.clone());
            }
            Decoded::Rename(rename) => {
                let description = rename.description.as_deref();
                let description_checked = description.unwrap_or(&record.description);
                if let Some(problem) = named::name_problem(&rename.name)
                    .or_else(|| named::description_problem(description_checked))
                {
                    return unprocessable("invalid_workspace", problem);
                }
                plan.name = rename.name.clone();
                plan.description = description_checked.to_string();
            }
            Decoded::Delete => {}
            Decoded::Operators(operators) => {
                let add: Vec<(String, String)> = (operators.add.iter())
                    .map(|o| (o.subject.clone(), o.login.clone()))
                    .collect();
                if !ops::operators_valid(&add, &operators.remove) {
                    return unprocessable(
                        "invalid_operators",
                        Msg::WorkspacesOperators("invalid").text(),
                    );
                }
                plan.add = add;
                plan.remove = operators.remove.clone();
            }
        }
        // A cheap look at the admission cache before anything is stored; the transaction decides
        // again from the newest set. A refusal is recorded all the same (ADR 0021).
        let version = plan.version.clone();
        if !plan.changed.is_empty()
            && let Some((answer, message)) = self.precheck(session, &plan.changed, &naming)
        {
            super::super::write::audit(session.subject.as_deref(), None, action, id, &message);
            let actor = super::super::write::actor(session, None);
            self.audit
                .refused(&actor, action, &arguments(version.as_deref()), &message);
            return answer;
        }
        // An apply that goes back to a version that was current is recorded as a rollback.
        let mut recorded = plan.action;
        if matches!(request, Decoded::Apply(_)) {
            let version = plan.version.as_deref().unwrap_or_default();
            let was = crate::modules::block_on(ops::was_current(policy.postgres, id, version));
            match was.map_err(|e| e.to_string()).and_then(|r| r) {
                Ok(true) => recorded = "workspaces.rollback",
                Ok(false) => {}
                Err(why) => return self.unreadable(session, action, id, &why),
            }
        }
        let record_pending = match self.pending(
            session,
            (recorded, id),
            recorded,
            arguments(version.as_deref()),
        ) {
            Ok(record) => record,
            Err(answer) => return *answer,
        };
        // The version document is the shared store's, immutable and content-addressed: written
        // before the rows that name it, so no row ever names a document that is not there.
        if let (Some(version), Some(document)) = (&plan.version, &plan.document)
            && let Err(why) = self.store_version(version, document)
        {
            return self.failed(record_pending, session, recorded, id, &why);
        }
        let actor = store::Creator {
            subject,
            login: session.login.as_deref(),
            administrator,
        };
        let target = Target {
            id,
            base,
            current: &record.current,
        };
        let version_text = plan.version.clone().unwrap_or_default();
        let op = match &request {
            Decoded::Save(_) => Op::Save {
                version: &version_text,
                message: &plan.message,
                identities: &plan.identities,
                changed: &plan.changed,
            },
            Decoded::Apply(_) => Op::Apply {
                version: &version_text,
                identities: &plan.identities,
                changed: &plan.changed,
            },
            Decoded::Rename(_) => Op::Rename {
                name: &plan.name,
                description: &plan.description,
            },
            Decoded::Delete => Op::Delete,
            Decoded::Operators(_) => Op::Operators {
                add: &plan.add,
                remove: &plan.remove,
            },
        };
        let done = crate::modules::block_on(ops::transact(&policy, &actor, &naming, &target, &op));
        match done {
            Ok(Ok(done)) => {
                let line = format!("{id} {} {}", done.action, version_text)
                    .trim_end()
                    .to_string();
                super::super::write::audit(session.subject.as_deref(), None, recorded, id, &line);
                finish(record_pending, Outcome::Accepted, &line, None);
                let mut body = json!({"base": done.seq});
                match &request {
                    Decoded::Save(_) => {
                        body["version"] = version_text.into();
                        body["saved"] = done.saved.into();
                    }
                    Decoded::Apply(_) => {
                        body["version"] = version_text.into();
                        body["action"] = done.action.into();
                    }
                    Decoded::Delete => body["deleted"] = true.into(),
                    _ => {}
                }
                let status = match &request {
                    Decoded::Save(_) if done.saved => StatusCode::CREATED,
                    _ => StatusCode::OK,
                };
                json_answer(status, &body)
            }
            Ok(Err(e)) => self.refused_by(record_pending, session, recorded, id, e),
            Err(e) => self.failed(record_pending, session, recorded, id, &e.to_string()),
        }
    }

    /// The answer to a read of Postgres or the store that failed before the pending record: the
    /// 503, logged without the reason's text on the wire.
    fn unreadable(&self, session: &Session, action: &str, id: &str, why: &str) -> Answer {
        eprintln!(
            "ui: {}",
            Msg::PostgresUnreachable(why).text().escape_debug()
        );
        super::super::write::audit(session.subject.as_deref(), None, action, id, "failed");
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            &Msg::WorkspacesUnavailable.text(),
        )
    }

    /// The refusal an operation's transaction returned, recorded and answered.
    fn refused_by(
        &self,
        record: PendingRecord,
        session: &Session,
        action: &str,
        id: &str,
        error_: Error,
    ) -> Answer {
        let (answer, outcome) = match error_ {
            Error::Failed(why) => return self.failed(record, session, action, id, &why),
            Error::Db(_) => return self.failed(record, session, action, id, "database"),
            Error::Pending(reason) => (super::pending(reason), format!("pending ({reason})")),
            Error::Refused(decided) => (
                super::entries_refused(&decided),
                super::refusal_line(&decided),
            ),
            Error::NotFound => {
                let message = Msg::WorkspacesNotFound.text();
                (Self::no_workspace(), message)
            }
            Error::Stale(seq) => {
                let message = Msg::WorkspacesStale(seq).text();
                let body = json!({"error": {
                    "code": "stale_base", "message": message, "base": seq,
                }});
                (json_answer(StatusCode::CONFLICT, &body), message)
            }
            Error::NotOperator => {
                let message = Msg::WorkspacesNotOperator.text();
                let answer = error(StatusCode::FORBIDDEN, "not_operator", &message);
                (answer, message)
            }
            Error::NotAdministrator => {
                let message = Msg::WorkspacesNotAdministrator.text();
                let answer = error(StatusCode::FORBIDDEN, "not_administrator", &message);
                (answer, message)
            }
            Error::UnknownVersion => {
                let message = Msg::WorkspacesUnknownVersion.text();
                let answer = error(StatusCode::NOT_FOUND, "unknown_version", &message);
                (answer, message)
            }
            Error::AlreadyCurrent => {
                let message = Msg::WorkspacesAlreadyCurrent.text();
                let answer = error(StatusCode::CONFLICT, "already_current", &message);
                (answer, message)
            }
            Error::Operators(reason) => {
                let message = Msg::WorkspacesOperators(reason).text();
                let answer = error(StatusCode::UNPROCESSABLE_ENTITY, reason, &message);
                (answer, message)
            }
        };
        super::super::write::audit(session.subject.as_deref(), None, action, id, &outcome);
        finish(record, Outcome::Refused, &outcome, None);
        answer
    }

    /// `GET workspaces/<id>/versions[?after=]`, `.../versions/<version>[?against=]` and
    /// `.../history[?after=]`; `None` when `rest` is none of them.
    pub(super) fn inspect(
        &self,
        rest: &str,
        query: Option<&str>,
        session: &Session,
    ) -> Option<Answer> {
        let (id, tail) = rest.split_once('/')?;
        let reads = match tail {
            "versions" | "history" => true,
            _ => tail
                .strip_prefix("versions/")
                .is_some_and(|v| !v.contains('/')),
        };
        reads.then_some(())?;
        if !meada_core::ids::is_id(id) {
            return Some(Self::no_workspace());
        }
        let record = match self.visible_records(session, Some(id)) {
            Ok(mut records) => match records.pop() {
                Some(record) => record,
                None => return Some(Self::no_workspace()),
            },
            Err(answer) => return Some(*answer),
        };
        Some(match tail {
            "versions" => self.versions(&record, query, session),
            "history" => self.history(&record, query, session),
            _ => {
                let version = tail.strip_prefix("versions/").unwrap_or_default();
                self.show_version(&record, version, query, session)
            }
        })
    }

    /// The one query parameter these reads take, `name`, once, within its bound.
    fn cursor(query: Option<&str>, name: &str) -> std::result::Result<Option<String>, Box<Answer>> {
        let mut found = None;
        for pair in query.into_iter().flat_map(|q| q.split('&')) {
            match pair.split_once('=') {
                Some((key, value))
                    if key == name && found.is_none() && value.len() <= AFTER_MAX =>
                {
                    found = Some(percent_decode_str(value).decode_utf8_lossy().into_owned());
                }
                _ => {
                    let message = Msg::WorkspacesPageQuery(AFTER_MAX).text();
                    return Err(Box::new(error(
                        StatusCode::BAD_REQUEST,
                        "bad_request",
                        &message,
                    )));
                }
            }
        }
        Ok(found)
    }

    /// The entries of `entries` that `session` reads, which are all of them for the platform
    /// administrator's management view.
    fn readable_entries(session: &Session, entries: Vec<Value>) -> Vec<Value> {
        let grant = Self::read_grant(session);
        (entries.into_iter())
            .filter(|e| session.operator || e["repo"].as_str().is_some_and(|r| grant.reads(r)))
            .collect()
    }

    /// Whether `session` reads a version whose document holds `identities`: the platform
    /// administrator and the operators see every version (filtered like the current one), anyone
    /// else only the versions it reads whole.
    fn reads_version(session: &Session, record: &Record, identities: &[String]) -> bool {
        Self::operates(session, record)
            || named::visible(false, identities, &Self::read_grant(session))
    }

    fn versions(&self, record: &Record, query: Option<&str>, session: &Session) -> Answer {
        let after = match Self::cursor(query, "after") {
            Ok(after) => after,
            Err(answer) => return *answer,
        };
        if after.as_deref().is_some_and(|a| !ops::cursor_ok(a)) {
            let message = Msg::WorkspacesPageQuery(AFTER_MAX).text();
            return error(StatusCode::BAD_REQUEST, "bad_request", &message);
        }
        let Some(policy) = self.workspace_policy() else {
            return Self::no_workspace();
        };
        let operates = Self::operates(session, record);
        let grant = Self::read_grant(session);
        let only: Option<BTreeSet<String>> = match (&grant, operates) {
            (Grant::Only(set), false) => Some(set.clone()),
            _ => None,
        };
        let read = crate::modules::block_on(ops::versions(
            policy.postgres,
            &record.id,
            only.as_ref(),
            after.as_deref(),
            ops::PAGE_MAX + 1,
        ));
        let mut rows = match read.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(rows) => rows,
            Err(why) => return self.unreadable(session, "workspaces.versions", &record.id, &why),
        };
        let next = (rows.len() > ops::PAGE_MAX).then(|| rows[ops::PAGE_MAX - 1].cursor.clone());
        rows.truncate(ops::PAGE_MAX);
        let versions: Vec<Value> = (rows.iter())
            .map(|row| {
                let mut shown = json!({
                    "version": row.version,
                    "saved": row.saved,
                    "message": row.message,
                    "current": row.current,
                });
                if operates {
                    shown["author"] = row.author.clone().into();
                }
                shown
            })
            .collect();
        json_answer(
            StatusCode::OK,
            &json!({"base": record.seq, "versions": versions, "next": next}),
        )
    }

    fn show_version(
        &self,
        record: &Record,
        version: &str,
        query: Option<&str>,
        session: &Session,
    ) -> Answer {
        let against = match Self::cursor(query, "against") {
            Ok(against) => against,
            Err(answer) => return *answer,
        };
        let Some(policy) = self.workspace_policy() else {
            return Self::no_workspace();
        };
        if !meada_core::ids::is_id(version) {
            return Self::no_workspace();
        }
        let row = crate::modules::block_on(ops::version(policy.postgres, &record.id, version));
        let row = match row.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(Some(row)) if Self::reads_version(session, record, &row.entries) => row,
            Ok(_) => {
                return error(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    &Msg::WorkspacesUnknownVersion.text(),
                );
            }
            Err(why) => return self.unreadable(session, "workspaces.versions", &record.id, &why),
        };
        let entries = match self.document(version) {
            Ok(entries) => entries,
            Err(why) => return self.unreadable(session, "workspaces.versions", &record.id, &why),
        };
        let operates = Self::operates(session, record);
        let total = entries.len();
        let shown = Self::readable_entries(session, entries);
        let mut document = json!({
            "id": row.version,
            "saved": row.saved,
            "message": row.message,
            "current": row.current,
            "base": record.seq,
            "entries": shown.iter().map(|entry| json!({
                "repo": entry["repo"],
                "blocks": crate::workspaces::Context::blocks(entry),
            })).collect::<Vec<_>>(),
            "text": meada_core::workspace::entries_text(&shown).unwrap_or_default(),
        });
        if operates {
            document["author"] = row.author.into();
            document["hidden"] = (total - shown.len()).into();
        }
        if let Some(against) = against {
            let other = match against.as_str() {
                "current" => record.current.clone(),
                other => other.to_string(),
            };
            if !meada_core::ids::is_id(&other) {
                let message = Msg::WorkspacesPageQuery(AFTER_MAX).text();
                return error(StatusCode::BAD_REQUEST, "bad_request", &message);
            }
            let known = crate::modules::block_on(ops::version(policy.postgres, &record.id, &other));
            let before = match known.map_err(|e| e.to_string()).and_then(|r| r) {
                Ok(Some(row)) if Self::reads_version(session, record, &row.entries) => {
                    match self.document(&other) {
                        Ok(entries) => Self::readable_entries(session, entries),
                        Err(why) => {
                            return self.unreadable(
                                session,
                                "workspaces.versions",
                                &record.id,
                                &why,
                            );
                        }
                    }
                }
                Ok(_) => {
                    return error(
                        StatusCode::NOT_FOUND,
                        "not_found",
                        &Msg::WorkspacesUnknownVersion.text(),
                    );
                }
                Err(why) => {
                    return self.unreadable(session, "workspaces.versions", &record.id, &why);
                }
            };
            // The diff names only entries the caller reads, hidden ones are neither added nor
            // removed in it.
            let diff = named::diff(&before, &shown);
            document["diff"] = json!({
                "against": other,
                "added": diff.added,
                "removed": diff.removed,
                "changed": diff.changed,
            });
        }
        json_answer(StatusCode::OK, &document)
    }

    fn history(&self, record: &Record, query: Option<&str>, session: &Session) -> Answer {
        if !Self::operates(session, record) {
            let message = Msg::WorkspacesNotOperator.text();
            return error(StatusCode::FORBIDDEN, "not_operator", &message);
        }
        let before = match Self::cursor(query, "after") {
            Ok(None) => None,
            Ok(Some(after)) => match after.parse::<i64>() {
                Ok(seq) if seq >= 1 && after.len() <= 19 => Some(seq),
                _ => {
                    let message = Msg::WorkspacesPageQuery(AFTER_MAX).text();
                    return error(StatusCode::BAD_REQUEST, "bad_request", &message);
                }
            },
            Err(answer) => return *answer,
        };
        let Some(policy) = self.workspace_policy() else {
            return Self::no_workspace();
        };
        let read = crate::modules::block_on(ops::histories(
            policy.postgres,
            &record.id,
            before,
            ops::PAGE_MAX + 1,
        ));
        let mut rows = match read.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(rows) => rows,
            Err(why) => return self.unreadable(session, "workspaces.history", &record.id, &why),
        };
        let next = (rows.len() > ops::PAGE_MAX).then(|| rows[ops::PAGE_MAX - 1].seq);
        rows.truncate(ops::PAGE_MAX);
        let history: Vec<Value> = (rows.iter())
            .map(|row| {
                json!({
                    "seq": row.seq, "action": row.action, "version": row.version,
                    "actor": row.actor, "authority": row.authority, "time": row.time,
                    "message": row.message,
                })
            })
            .collect();
        json_answer(
            StatusCode::OK,
            &json!({"base": record.seq, "history": history, "next": next}),
        )
    }
}

/// The decoded body of a lifecycle write.
enum Decoded {
    Save(Save),
    Apply(Apply),
    Rename(Rename),
    Delete,
    Operators(Operators),
}
