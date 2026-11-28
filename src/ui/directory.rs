use super::*;

fn unreadable(reason: &str) {
    eprintln!(
        "ui: {}",
        Msg::DirectoryUnreadable(reason).text().escape_debug()
    );
}

fn blocking<T>(
    directory: &crate::directory::read::Directory,
    ask: impl Future<Output = std::result::Result<T, String>>,
) -> Option<T> {
    match crate::modules::within(directory.bound(), ask, Msg::PostgresTimeout) {
        Ok(answer) => Some(answer),
        Err(reason) => {
            unreadable(&reason);
            None
        }
    }
}

impl Console {
    #[cfg_attr(not(feature = "oidc"), expect(dead_code))]
    pub(super) fn directory_refusal(
        &self,
        user: Option<&str>,
        write: bool,
    ) -> std::result::Result<bool, (Box<Answer>, String)> {
        let (Some(directory), Some(user)) = (self.directory.as_ref(), user) else {
            return Ok(false);
        };
        match blocking(directory, directory.deprovisioned(user)) {
            Some(false) => Ok(false),
            Some(true) => {
                let message = Msg::UiDeprovisioned.text();
                let answer = error(StatusCode::FORBIDDEN, "deprovisioned", &message);
                Err((Box::new(answer), message))
            }
            None if write => {
                let message = Msg::AuditUnavailable.text();
                let status = StatusCode::SERVICE_UNAVAILABLE;
                let answer = error(status, "audit_unavailable", &message);
                Err((Box::new(answer), String::new()))
            }
            None => Ok(true),
        }
    }

    pub(super) fn directory_groups(&self, query: Option<&str>, session: &Session) -> Answer {
        if !session.operator {
            let message = Msg::UiOperatorRequired.text();
            return error(StatusCode::FORBIDDEN, "forbidden", &message);
        }
        let Some(directory) = &self.directory else {
            return error(StatusCode::NOT_FOUND, "not_found", &Msg::UiNoRoute.text());
        };
        let max = meada_core::manifest::OWNER_MAX;
        let pairs: Vec<&str> =
            (query.filter(|q| !q.is_empty())).map_or(Vec::new(), |q| q.split('&').collect());
        let prefix = match pairs.as_slice() {
            [] => Some(String::new()),
            [pair] => (pair.strip_prefix("prefix=")).and_then(|text| {
                let decoded = percent_decode_str(text).decode_utf8().ok()?;
                (decoded.chars().count() <= max).then(|| decoded.into_owned())
            }),
            _ => None,
        };
        let Some(prefix) = prefix else {
            let message = Msg::UiDirectoryQuery(max).text();
            return error(StatusCode::BAD_REQUEST, "bad_request", &message);
        };
        if prefix.contains('\0') {
            return json_answer(StatusCode::OK, &json!({"groups": []}));
        }
        let groups = if session.directory_down {
            None
        } else {
            blocking(directory, directory.groups(&prefix))
        };
        match groups {
            Some(groups) => json_answer(StatusCode::OK, &json!({"groups": groups})),
            None => {
                let message = Msg::DirectoryUnavailable.text();
                error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message)
            }
        }
    }

    #[cfg_attr(not(feature = "generation-pointer"), expect(dead_code))]
    pub(super) async fn flag_missing_owners(&self, entries: &mut [Value], session: &Session) {
        let Some(directory) = &self.directory else {
            return;
        };
        if session.directory_down {
            return;
        }
        let owners: std::collections::BTreeSet<&str> =
            entries.iter().filter_map(|e| e["owner"].as_str()).collect();
        if owners.is_empty() {
            return;
        }
        let owners: Vec<String> = owners.into_iter().map(str::to_string).collect();
        let named = match directory.named(&owners).await {
            Ok(Some(named)) => named,
            Ok(None) => return,
            Err(reason) => return unreadable(&reason),
        };
        for entry in entries {
            if entry["owner"].as_str().is_some_and(|o| !named.contains(o)) {
                entry["owner_missing"] = true.into();
            }
        }
    }

    #[cfg_attr(not(feature = "oidc"), expect(dead_code))]
    pub(super) fn with_directory_counts(&self, document: &mut Value, session: &Session) {
        let Some(directory) = &self.directory else {
            return;
        };
        let counts = if session.directory_down {
            None
        } else {
            blocking(directory, directory.counts())
        };
        document["directory"] = match counts {
            Some(counts) => json!({
                "users": counts.users,
                "groups": counts.groups,
                "last_write": counts.last_write,
            }),
            None => Value::Null,
        };
    }
}
