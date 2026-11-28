use std::collections::BTreeMap;

use meada_core::effective::{self, Effective, Facts};

use super::*;
use crate::workspaces::lifecycle as ops;

impl Console {
    fn facts(
        entries: &[Value],
        held: Option<&crate::snapshot::serve::Held>,
        excluded: &BTreeSet<String>,
        session: &Session,
    ) -> BTreeMap<String, Facts> {
        let mut facts = BTreeMap::new();
        for repo in entries.iter().filter_map(|e| e["repo"].as_str()) {
            let mut found = match held {
                Some(held) if session.reads(repo) => held.facts(repo).unwrap_or_default(),
                _ => Facts::default(),
            };
            found.excluded = excluded.contains(repo);
            facts.insert(repo.to_string(), found);
        }
        facts
    }

    pub(super) fn view(
        entries: &[Value],
        held: Option<&crate::snapshot::serve::Held>,
        excluded: &BTreeSet<String>,
        session: &Session,
    ) -> Vec<Effective> {
        effective::effective(entries, &Self::facts(entries, held, excluded, session))
    }

    fn diagnostics(view: &[Effective]) -> Value {
        let mut list = Vec::new();
        for entry in view {
            for diagnostic in &entry.diagnostics {
                let mut found = json!({"repo": entry.repo, "code": diagnostic.code});
                if let Some(detail) = &diagnostic.detail {
                    found["detail"] = detail.clone().into();
                }
                list.push(found);
            }
        }
        Value::Array(list)
    }

    pub(super) fn written_diagnostics(
        &self,
        record_id: &str,
        version: &str,
        session: &Session,
    ) -> Value {
        let Ok(entries) = self.document(version) else {
            return json!([]);
        };
        let entries = Self::readable_entries(session, entries);
        let held = self.held_generation();
        let excluded = self.excluded_for(record_id, true);
        Self::diagnostics(&Self::view(&entries, held.as_deref(), &excluded, session))
    }

    pub(super) fn excluded_for(&self, id: &str, operates: bool) -> BTreeSet<String> {
        match operates {
            true => self.providers.excluded_in(id),
            false => BTreeSet::new(),
        }
    }

    pub(super) fn configuration(
        &self,
        record: &Record,
        query: Option<&str>,
        session: &Session,
    ) -> Answer {
        let requested = match Self::cursor(query, "version") {
            Ok(requested) => requested,
            Err(answer) => return *answer,
        };
        let Some(policy) = self.workspace_policy() else {
            return Self::no_workspace();
        };
        let version = requested.unwrap_or_else(|| record.current.clone());
        let unknown = || {
            error(
                StatusCode::NOT_FOUND,
                "not_found",
                &Msg::WorkspacesUnknownVersion.text(),
            )
        };
        if !meada_core::ids::is_id(&version) {
            return unknown();
        }
        if version != record.current {
            let row = crate::modules::block_on(ops::version(policy.postgres, &record.id, &version));
            match row.map_err(|e| e.to_string()).and_then(|r| r) {
                Ok(Some(row)) if Self::reads_version(session, record, &row.entries) => {}
                Ok(_) => return unknown(),
                Err(why) => {
                    return self.unreadable(session, "workspaces.effective", &record.id, &why);
                }
            }
        }
        let entries = match self.document(&version) {
            Ok(entries) => entries,
            Err(why) => {
                return self.unreadable(session, "workspaces.effective", &record.id, &why);
            }
        };
        let operates = Self::operates(session, record);
        let total = entries.len();
        let shown = Self::readable_entries(session, entries);
        let held = self.held_generation();
        let excluded = self.excluded_for(&record.id, operates);
        let view = Self::view(&shown, held.as_deref(), &excluded, session);
        let mut document = json!({
            "workspace": record.id,
            "version": version,
            "current": version == record.current,
            "base": record.seq,
            "state": Self::state(record, held.as_deref()),
            "recorded": held.as_ref().is_some_and(|h| h.records(&record.id, &version)),
            "generation": held.as_ref().map(|h| h.id.clone()),
            "entries": view,
        });
        if operates {
            document["hidden"] = (total - shown.len()).into();
        }
        json_answer(StatusCode::OK, &document)
    }

    pub(super) fn preview(&self, body: &Body, session: &Session) -> Answer {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Preview {
            entries: String,
        }
        let Some(section) = &self.bootstrap.workspaces else {
            return Self::no_workspace();
        };
        let bad = |status, code: &str, message: &str| error(status, code, message);
        let body = match body {
            Ok(body) => body,
            Err(StatusCode::PAYLOAD_TOO_LARGE) => {
                let message = Msg::UiTooLarge(super::CREATE_MAX).text();
                return bad(StatusCode::PAYLOAD_TOO_LARGE, "too_large", &message);
            }
            Err(_) => {
                let message = Msg::WorkspacesBody.text();
                return bad(StatusCode::BAD_REQUEST, "bad_request", &message);
            }
        };
        let Ok(preview) = serde_json::from_slice::<Preview>(body) else {
            let message = Msg::WorkspacesBody.text();
            return bad(StatusCode::BAD_REQUEST, "bad_request", &message);
        };
        let entries = match named::version_entries("entries", &preview.entries, section.max_entries)
        {
            Ok(entries) => entries,
            Err(problems) => {
                return bad(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_entries",
                    &problems.join("\n"),
                );
            }
        };
        let held = self.held_generation();
        let view = Self::view(&entries, held.as_deref(), &BTreeSet::new(), session);
        json_answer(
            StatusCode::OK,
            &json!({
                "workspace": null,
                "generation": held.as_ref().map(|h| h.id.clone()),
                "entries": view,
            }),
        )
    }
}
