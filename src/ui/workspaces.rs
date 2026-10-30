//! Named workspaces in the console (#342, ADR 0021 contract 4): create, list and show under the
//! authenticated subject, and the playground and export through a context. Identity, role and
//! grant come from the token and the permissions set, never from the body: a caller-supplied
//! actor or role is an unknown key and grants nothing. Everything a read returns follows the
//! discovery rule of ADR 0020: a workspace the caller may not see is the one 404 of an id that
//! does not exist, with the same body, so no read tells them apart.

use std::collections::BTreeSet;

use meada_core::authorize::Grant;
use meada_core::named;

mod lifecycle;

use super::write::{PendingRecord, finish, refused};
use super::*;
use crate::audit::Outcome;
use crate::workspaces::store::{self, Creator, New, Policy, Record};
pub(super) use lifecycle::body_max;

// ponytail: a creation carries a manifest text of at most 64 KiB, a name, a description and a
// message, with room for JSON's escapes.
pub(super) const CREATE_MAX: usize = 80 << 10;

// ponytail: the most workspaces a page lists, and how many without `limit`; the longest cursor
// `after` the list reads (an id, with room to spare).
const LIST_LIMIT_MAX: usize = 100;
const LIST_LIMIT: usize = 25;

const AFTER_MAX: usize = 256;

/// The action a creation's audit record names.
const CREATE: &str = "workspaces.create";

impl Console {
    /// The answer at `workspaces[/...]`, or `None` when the route is not one of named workspaces
    /// or the console has no `[workspaces]` (the route then does not exist, as before #342).
    pub(super) fn workspaces(
        &self,
        request: &Request<Body>,
        route: &str,
        session: &Session,
    ) -> Option<Answer> {
        self.bootstrap.workspaces.as_ref()?;
        let rest = route.strip_prefix("workspaces")?;
        let read = matches!(*request.method(), Method::GET | Method::HEAD);
        let post = *request.method() == Method::POST;
        match rest.strip_prefix('/') {
            None if rest.is_empty() && post => Some(self.create(request.body(), session)),
            None if rest.is_empty() && read => Some(self.list(request.uri().query(), session)),
            Some(rest) if post => self.operate(rest, request.body(), session),
            Some(rest) if read => {
                // An identity holds slashes: everything between `repositories/` and `/export`.
                if let Some((id, tail)) = rest.split_once("/repositories/")
                    && let Some(identity) = tail.strip_suffix("/export")
                {
                    return Some(self.export_through(id, identity, request.uri().query(), session));
                }
                if let Some(answer) = self.inspect(rest, request.uri().query(), session) {
                    return Some(answer);
                }
                (!rest.contains('/')).then(|| self.show(rest, session))
            }
            _ => None,
        }
    }

    /// The one 404 of a workspace that does not exist, was deleted or the caller may not see.
    fn no_workspace() -> Answer {
        error(
            StatusCode::NOT_FOUND,
            "not_found",
            &Msg::WorkspacesNotFound.text(),
        )
    }

    /// The postgres section and `[oidc.github]` the routes read: `[workspaces]` required both.
    fn workspace_policy(&self) -> Option<Policy<'_>> {
        let postgres = self.bootstrap.postgres.as_ref()?;
        let github = (self.bootstrap.oidc.as_ref()).and_then(|o| o.github.as_ref())?;
        let section = self.bootstrap.workspaces.as_ref()?;
        Some(Policy {
            postgres,
            max_age: github.max_age,
            roles: &github.roles,
            max_workspaces: section.max_workspaces,
        })
    }

    /// Whether `session` operates workspace `record`: the platform administrator, or an operator
    /// of it by subject.
    fn operates(session: &Session, record: &Record) -> bool {
        session.operator
            || (session.subject.as_deref())
                .is_some_and(|s| record.operators.iter().any(|(subject, _)| subject == s))
    }

    /// What `session` reads, as `meada-core` takes it.
    fn read_grant(session: &Session) -> Grant {
        #[cfg(feature = "oidc")]
        if let Some(crate::oidc::Grant::Only(identities)) = &session.grant {
            return Grant::Only(identities.iter().cloned().collect());
        }
        Grant::All
    }

    /// `POST workspaces`: `{"name", "description"?, "entries", "message"}`, the manifest text of
    /// the selection. The creator is the authenticated subject; the selection must be eligible for
    /// its login in the newest permissions set, decided again inside the transaction that inserts
    /// the workspace. 201 `{"id", "version", "created": true}`, or 200 with `created: false`
    /// when the same creator, name and entries made it already.
    fn create(&self, body: &Body, session: &Session) -> Answer {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Create {
            name: String,
            #[serde(default)]
            description: String,
            entries: String,
            message: String,
        }
        let (Some(section), Some(policy)) = (&self.bootstrap.workspaces, self.workspace_policy())
        else {
            return Self::no_workspace();
        };
        let bad = |status, code, message: &str| {
            refused(None, session, CREATE, "-", status, code, message)
        };
        let body = match body {
            Ok(body) => body,
            Err(StatusCode::PAYLOAD_TOO_LARGE) => {
                let message = Msg::UiTooLarge(CREATE_MAX).text();
                return bad(StatusCode::PAYLOAD_TOO_LARGE, "too_large", &message);
            }
            Err(_) => {
                return bad(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    &Msg::WorkspacesBody.text(),
                );
            }
        };
        let Ok(create) = serde_json::from_slice::<Create>(body) else {
            return bad(
                StatusCode::BAD_REQUEST,
                "bad_request",
                &Msg::WorkspacesBody.text(),
            );
        };
        let subject = match session.subject.as_deref() {
            Some(subject) if !subject.is_empty() => subject,
            _ => {
                let message = Msg::UiOperatorRequired.text();
                return bad(StatusCode::FORBIDDEN, "forbidden", &message);
            }
        };
        let unprocessable =
            |code, message: &str| bad(StatusCode::UNPROCESSABLE_ENTITY, code, message);
        if let Some(problem) = named::name_problem(&create.name)
            .or_else(|| named::description_problem(&create.description))
            .or_else(|| (create.message.is_empty()).then(|| Msg::WorkspacesBody.text()))
            .or_else(|| named::description_problem(&create.message))
        {
            return unprocessable("invalid_workspace", &problem);
        }
        let entries = match named::version_entries("entries", &create.entries, section.max_entries)
        {
            Ok(entries) if !entries.is_empty() => entries,
            Ok(_) => {
                let message = Msg::UiEntriesInvalid.text();
                return unprocessable("invalid_entries", &message);
            }
            Err(problems) => return unprocessable("invalid_entries", &problems.join("\n")),
        };
        let version = named::version_id(&entries);
        let selected = named::identities(&entries);
        let administrator = session.operator;
        let login = session.login.as_deref();
        let grant = Self::read_grant(session);
        let id = meada_core::ids::workspace_id(subject, &create.name, &version);
        let authority = match administrator {
            true => "platform_administrator",
            false => "workspace_operator",
        };
        let arguments = json!({"workspace": id, "version": version, "authority": authority});
        // A cheap look at the admission cache before anything is stored; the transaction below
        // decides again from the newest set. A refusal is recorded all the same (ADR 0021).
        if let Some((answer, message)) = self.precheck(session, &selected, &grant) {
            super::write::audit(session.subject.as_deref(), None, CREATE, &id, &message);
            let actor = super::write::actor(session, None);
            self.audit.refused(&actor, CREATE, &arguments, &message);
            return answer;
        }
        let record = match self.pending(session, (CREATE, &id), CREATE, arguments) {
            Ok(record) => record,
            Err(answer) => return *answer,
        };
        // The version document is the shared store's, immutable and content-addressed: written
        // before the rows that name it, so no row ever names a document that is not there.
        if let Err(why) = self.store_version(&version, &entries) {
            return self.failed(record, session, CREATE, &id, &why);
        }
        let creator = Creator {
            subject,
            login,
            administrator,
        };
        let new = New {
            name: &create.name,
            description: &create.description,
            version: &version,
            entries: &entries,
            message: &create.message,
        };
        let created = crate::modules::block_on(store::create(&policy, &creator, &grant, &new));
        let created = match created {
            Ok(created) => created,
            Err(e) => return self.failed(record, session, CREATE, &id, &e.to_string()),
        };
        match created {
            Ok(created) => {
                let line = format!("{} {}", created.id, version);
                super::write::audit(session.subject.as_deref(), None, CREATE, &created.id, &line);
                finish(record, Outcome::Accepted, &line, None);
                let status = match created.created {
                    true => StatusCode::CREATED,
                    false => StatusCode::OK,
                };
                json_answer(
                    status,
                    &json!({"id": created.id, "version": version, "created": created.created}),
                )
            }
            Err(e) => self.refusal(record, session, &id, e),
        }
    }

    /// The refusal of a creation by the admission cache's catalog, before anything is stored, with
    /// the message its record carries: `None` when the cache lets the selection pass (or cannot
    /// say: the transaction decides).
    fn precheck(
        &self,
        session: &Session,
        selected: &[String],
        grant: &Grant,
    ) -> Option<(Answer, String)> {
        let oidc = self.oidc.as_ref()?;
        let catalog = oidc.catalog(session.login.as_deref(), session.operator)?;
        if let Some(reason) = catalog.reason {
            return Some((pending(reason), format!("pending ({reason})")));
        }
        let decided = named::creation(selected, &catalog.selectable, grant);
        (!decided.allowed).then(|| (entries_refused(&decided), refusal_line(&decided)))
    }

    /// Stores the version document `entries` is, unless the store holds it already.
    fn store_version(&self, version: &str, entries: &[Value]) -> std::result::Result<(), String> {
        let Some(snapshot) = &self.bootstrap.snapshot else {
            return Err(Msg::SnapshotNoSection(&self.file.display().to_string()).text());
        };
        store::put_document(snapshot, version, entries)
    }

    fn failed(
        &self,
        record: PendingRecord,
        session: &Session,
        action: &str,
        id: &str,
        why: &str,
    ) -> Answer {
        eprintln!(
            "ui: {}",
            Msg::PostgresUnreachable(why).text().escape_debug()
        );
        let message = Msg::AuditUnavailable.text();
        let _ = message;
        super::write::audit(session.subject.as_deref(), None, action, id, "failed");
        finish(record, Outcome::Failed, "failed", None);
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            &Msg::WorkspacesPending("unavailable").text(),
        )
    }

    fn refusal(
        &self,
        record: PendingRecord,
        session: &Session,
        id: &str,
        error_: store::CreateError,
    ) -> Answer {
        use store::CreateError;
        let (answer, outcome) = match error_ {
            CreateError::Pending(reason) => (pending(reason), format!("pending ({reason})")),
            CreateError::Refused(decided) => (entries_refused(&decided), refusal_line(&decided)),
            CreateError::Limit => {
                let max = self
                    .bootstrap
                    .workspaces
                    .as_ref()
                    .map_or(0, |s| s.max_workspaces);
                let message = Msg::WorkspacesLimit(max).text();
                (error(StatusCode::CONFLICT, "limit", &message), message)
            }
            CreateError::Deleted => {
                let message = Msg::WorkspaceDeleted.text();
                (
                    error(StatusCode::CONFLICT, "workspace_deleted", &message),
                    message,
                )
            }
            CreateError::Failed(why) => return self.failed(record, session, CREATE, id, &why),
        };
        super::write::audit(session.subject.as_deref(), None, CREATE, id, &outcome);
        finish(record, Outcome::Refused, &outcome, None);
        answer
    }

    /// The records of the workspaces the session sees, ordered by id: every workspace for the
    /// platform administrator, the ones it operates, and the ones whose entries its grant reads
    /// whole. Postgres unreachable is the 503 of the audit.
    fn visible_records(
        &self,
        session: &Session,
        only: Option<&str>,
    ) -> std::result::Result<Vec<Record>, Box<Answer>> {
        let (Some(policy), Some(section)) = (self.workspace_policy(), &self.bootstrap.workspaces)
        else {
            return Err(Box::new(Self::no_workspace()));
        };
        let read = crate::modules::block_on(store::records(
            policy.postgres,
            only,
            section.max_workspaces,
        ));
        let all = match read.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(all) => all,
            Err(why) => {
                eprintln!(
                    "ui: {}",
                    Msg::PostgresUnreachable(&why).text().escape_debug()
                );
                let message = Msg::AuditUnavailable.text();
                return Err(Box::new(error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unavailable",
                    &message,
                )));
            }
        };
        let grant = Self::read_grant(session);
        Ok(all
            .into_iter()
            .filter(|r| named::visible(Self::operates(session, r), &r.entries, &grant))
            .collect())
    }

    /// `GET workspaces?limit=&after=`: the workspaces the session sees, `limit` 1 to 100 (25 by
    /// default) after the id `after`, as `{"workspaces", "next"}`.
    fn list(&self, query: Option<&str>, session: &Session) -> Answer {
        let (mut limit, mut after) = (None, None);
        for pair in query.into_iter().flat_map(|q| q.split('&')) {
            let taken = match pair.split_once('=') {
                Some(("limit", n)) if limit.is_none() => {
                    limit = n
                        .parse::<usize>()
                        .ok()
                        .filter(|n| (1..=LIST_LIMIT_MAX).contains(n))
                        .map(Some);
                    limit.is_some()
                }
                Some(("after", id)) if after.is_none() && id.len() <= AFTER_MAX => {
                    after = meada_core::ids::is_id(id).then(|| Some(id.to_string()));
                    after.is_some()
                }
                _ => false,
            };
            if !taken {
                let message = Msg::WorkspacesQuery(AFTER_MAX).text();
                return error(StatusCode::BAD_REQUEST, "bad_request", &message);
            }
        }
        let limit = limit.flatten().unwrap_or(LIST_LIMIT);
        let after = after.flatten();
        let records = match self.visible_records(session, None) {
            Ok(records) => records,
            Err(answer) => return *answer,
        };
        let mut page: Vec<Record> = (records.into_iter())
            .filter(|r| after.as_deref().is_none_or(|a| r.id.as_str() > a))
            .take(limit + 1)
            .collect();
        let next = (page.len() > limit).then(|| page[limit - 1].id.clone());
        page.truncate(limit);
        let held = self.held_generation();
        let workspaces: Vec<Value> = (page.iter())
            .map(|r| self.summary(r, held.as_deref(), session))
            .collect();
        json_answer(
            StatusCode::OK,
            &json!({"workspaces": workspaces, "next": next}),
        )
    }

    /// The generation the console holds now, when it keeps generations and one is servable.
    fn held_generation(&self) -> Option<Arc<crate::snapshot::serve::Held>> {
        #[cfg(feature = "snapshot")]
        if let Holds::Generations(Ok(current)) = &self.holds {
            return current.get().ok();
        }
        None
    }

    /// `state`: `live` when the current generation records the workspace at its current version,
    /// else `applied`: stored, with its ingestion pending (ADR 0021).
    fn state(record: &Record, held: Option<&crate::snapshot::serve::Held>) -> &'static str {
        match held.and_then(|h| h.context_version(&record.id)) {
            Some(version) if version == record.current => "live",
            _ => "applied",
        }
    }

    /// One workspace as a list shows it.
    fn summary(
        &self,
        record: &Record,
        held: Option<&crate::snapshot::serve::Held>,
        session: &Session,
    ) -> Value {
        let grant = Self::read_grant(session);
        let operates = Self::operates(session, record);
        let readable = named::readable(&record.entries, &grant).len();
        let mut shown = json!({
            "id": record.id,
            "name": record.name,
            "description": record.description,
            "version": record.current,
            "state": Self::state(record, held),
            "entries": match session.operator {
                true => record.entries.len(),
                false => readable,
            },
            "operate": operates,
        });
        if operates && !session.operator {
            shown["hidden"] = (record.entries.len() - readable).into();
        }
        shown
    }

    /// `GET workspaces/<id>`: the workspace, its `base` (the fact a write must still find), its
    /// `state` and each entry's `ingestion`. Operators and the platform administrator also get
    /// the operator list and, for an operator that cannot read every entry, the count it
    /// cannot read (`hidden`): the entries it lists are the ones its grant reads.
    fn show(&self, id: &str, session: &Session) -> Answer {
        if !meada_core::ids::is_id(id) {
            return Self::no_workspace();
        }
        let record = match self.visible_records(session, Some(id)) {
            Ok(mut records) => match records.pop() {
                Some(record) => record,
                None => return Self::no_workspace(),
            },
            Err(answer) => return *answer,
        };
        let entries = match self.version_entries(&record) {
            Ok(entries) => entries,
            Err(why) => {
                eprintln!("ui: {}", why.escape_debug());
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "unavailable",
                    &Msg::WorkspacesDocumentBad(&record.id).text(),
                );
            }
        };
        let grant = Self::read_grant(session);
        let operates = Self::operates(session, &record);
        let held = self.held_generation();
        let live = Self::state(&record, held.as_deref()) == "live";
        let excluded: BTreeSet<String> = match &self.oidc {
            Some(oidc) => oidc.excluded_in(&record.id),
            None => BTreeSet::new(),
        };
        let recorded: Vec<&str> = held
            .as_ref()
            .and_then(|h| h.context_repositories(&record.id))
            .unwrap_or_default();
        // The platform administrator's management view is unfiltered; an operator edits the
        // entries it reads (ADR 0021).
        let shown: Vec<&Value> = (entries.iter())
            .filter(|e| session.operator || e["repo"].as_str().is_some_and(|r| grant.reads(r)))
            .collect();
        let rows: Vec<Value> = shown
            .iter()
            .map(|entry| {
                let repo = entry["repo"].as_str().unwrap_or_default();
                let ingestion = if operates && excluded.contains(repo) {
                    "excluded"
                } else if live && recorded.contains(&repo) {
                    "ready"
                } else {
                    "pending"
                };
                json!({
                    "repo": repo,
                    "ingestion": ingestion,
                    "blocks": crate::workspaces::Context::blocks(entry),
                })
            })
            .collect();
        let mut document = json!({
            "id": record.id,
            "name": record.name,
            "description": record.description,
            "created": record.created,
            "version": record.current,
            "base": record.seq,
            "state": Self::state(&record, held.as_deref()),
            "entries": rows,
            "operate": operates,
        });
        if operates {
            document["hidden"] = (entries.len() - shown.len()).into();
            document["operators"] = (record.operators.iter())
                .map(|(subject, login)| json!({"subject": subject, "login": login}))
                .collect();
        }
        json_answer(StatusCode::OK, &document)
    }

    /// The entries of the workspace's current version from the shared store, checked.
    fn version_entries(&self, record: &Record) -> std::result::Result<Vec<Value>, String> {
        self.document(&record.current)
    }

    /// The entries of the named version document `version` from the shared store, checked.
    fn document(&self, version: &str) -> std::result::Result<Vec<Value>, String> {
        let Some(snapshot) = &self.bootstrap.snapshot else {
            return Err(Msg::SnapshotNoSection(&self.file.display().to_string()).text());
        };
        store::get_document(snapshot, version)
    }

    /// The playground's and the export's workspace check: whether `session` may ask through
    /// workspace `id` about `repository` (ADR 0021): the context is in the generation, holds the
    /// repository, and the session operates it or reads every repository it records.
    pub(super) fn through_workspace(
        &self,
        id: &str,
        repository: &str,
        held: &crate::snapshot::serve::Held,
        session: &Session,
    ) -> bool {
        let Some(repos) = held.context_repositories(id) else {
            return false;
        };
        if !repos.contains(&repository) || !session.reads(repository) {
            return false;
        }
        // The workspace must exist, not be deleted and be visible to the session (it operates it
        // or reads it whole): the generation may retain a context Postgres no longer lists. Every
        // answer is bounded by the grant.
        match self.visible_records(session, Some(id)) {
            Ok(records) => !records.is_empty(),
            Err(_) => false,
        }
    }

    /// `GET workspaces/<id>/repositories/<identity>/export[?generation=]`: the export of the
    /// context, under the discovery rule.
    fn export_through(
        &self,
        id: &str,
        identity: &str,
        query: Option<&str>,
        session: &Session,
    ) -> Answer {
        let identity = percent_decode_str(identity).decode_utf8_lossy();
        let generation = (query.into_iter().flat_map(|q| q.split('&')))
            .find_map(|pair| pair.strip_prefix("generation="))
            .map(|id| percent_decode_str(id).decode_utf8_lossy());
        let engine = match self.open(&identity, generation.as_deref(), Some(id), session) {
            Ok((engine, _)) => engine,
            Err(refused) => return *refused,
        };
        match engine.export() {
            Ok(export) => Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Full::from(export).boxed())
                .expect("a valid response"),
            Err(e) => error(
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                &e.to_string(),
            ),
        }
    }
}

/// The 409 of a platform or caller condition that stops every selection (fail closed).
pub(super) fn pending(reason: &str) -> Answer {
    json_answer(
        StatusCode::CONFLICT,
        &json!({"error": {
            "code": "pending",
            "message": Msg::WorkspacesPending(reason).text(),
            "reason": reason,
        }}),
    )
}

/// The audit message of a refused selection: how many, the identities named, the count hidden.
pub(super) fn refusal_line(decided: &named::Creation) -> String {
    format!(
        "{} ({}; hidden {})",
        Msg::WorkspacesRefused(decided.named.len() + decided.hidden).text(),
        decided.named.join(", "),
        decided.hidden
    )
}

/// The 403 `entries_refused`: the refused identities the caller's grant reads, and the count of
/// the others, which are never named.
pub(super) fn entries_refused(decided: &named::Creation) -> Answer {
    let refused: Vec<Value> = (decided.named.iter())
        .map(|identity| json!({"identity": identity, "change": "add", "reason": "not_eligible"}))
        .collect();
    json_answer(
        StatusCode::FORBIDDEN,
        &json!({"error": {
            "code": "entries_refused",
            "message": Msg::WorkspacesRefused(decided.named.len() + decided.hidden).text(),
            "refused": refused,
            "hidden": decided.hidden,
        }}),
    )
}
