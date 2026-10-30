//! The effective configuration of a named workspace in the console (#344, ADR 0021 contract 6):
//! `GET /api/v1/workspaces/<id>/effective[?version=]` and the stateless
//! `POST /api/v1/workspaces/effective` that shows the same view for manifest text before it is
//! saved. Both decide with `meada_core::effective`, the function the browser calls too, from
//! facts the console reads from the generation it holds: whether a bundle exists, whether its
//! mirror has a `meada.toml` and whether its ingestion declares `[serve]`. Nothing here stores,
//! moves or audits anything, and a view names no repository the caller's grant does not read.

use std::collections::BTreeMap;

use meada_core::effective::{self, Effective, Facts};

use super::*;
use crate::workspaces::lifecycle as ops;

impl Console {
    /// The facts of each of `entries` (version-document entries) in the generation `held`, none
    /// for a repository `session` may not read or a generation that lacks it: the view then says
    /// the repository is pending, as it does for any repository no generation holds, so the
    /// facts are no oracle on repositories the caller cannot read. `excluded` marks the entries
    /// the newest permissions set excludes for the workspace.
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

    /// The view of `entries` as the caller reads them.
    fn view(
        entries: &[Value],
        held: Option<&crate::snapshot::serve::Held>,
        excluded: &BTreeSet<String>,
        session: &Session,
    ) -> Vec<Effective> {
        effective::effective(entries, &Self::facts(entries, held, excluded, session))
    }

    /// The diagnostics of `view` as `[{repo, code, detail?}]`, the entries with none left out:
    /// what an accepted write shows beside its result, so an ignored declaration never looks
    /// applied.
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

    /// The diagnostics of the version `version` of workspace `record` as `session` reads it, for
    /// the answer of an accepted write; `[]` when the document cannot be read, since the write
    /// has already happened and its answer must not turn into a failure.
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

    /// The identities the newest permissions set excludes from workspace `id`, for whoever
    /// operates it.
    fn excluded_for(&self, id: &str, operates: bool) -> BTreeSet<String> {
        match (&self.oidc, operates) {
            (Some(oidc), true) => oidc.excluded_in(id),
            _ => BTreeSet::new(),
        }
    }

    /// `GET workspaces/<id>/effective[?version=]`: the effective configuration of the workspace's
    /// current version, or of one of its versions, per entry the caller reads (the platform
    /// administrator's management view is unfiltered): `source`, the `blocks` that apply,
    /// `ignored`, `coverage` and `diagnostics`, with the generation the facts come from. Every
    /// refusal follows the discovery rule: the caller got here through `visible_records`.
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
            // Whether the held generation recorded the context at this very version: when it
            // did not, the blocks shown reach queries only after a refresh publishes them.
            "recorded": held.as_ref().is_some_and(|h| h.records(&record.id, &version)),
            "generation": held.as_ref().map(|h| h.id.clone()),
            "entries": view,
        });
        if operates {
            document["hidden"] = (total - shown.len()).into();
        }
        json_answer(StatusCode::OK, &document)
    }

    /// `POST workspaces/effective` `{"entries"}`: the view of manifest text as the console would
    /// show it once saved, before anything is saved. It is stateless: nothing is stored, audited
    /// or checked against eligibility (the save decides that), and a repository the caller's
    /// grant does not read is shown as pending, whatever the generation holds.
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
