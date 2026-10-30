//! The console's reads of the directory (#335, feature `directory`): deprovisioning at
//! admission, the owner picker, `owner_missing` and the admission counts. Every read is made per
//! request and opens its own connection; one that fails writes one `ui: directory: unreadable`
//! line (the scrubbed reason, never a claim) and the route goes on as the module's text says.

use super::*;

/// The stderr line of a directory that could not be read.
fn unreadable(reason: &str) {
    eprintln!(
        "ui: {}",
        Msg::DirectoryUnreadable(reason).text().escape_debug()
    );
}

/// `ask` run on a runtime of its own within `directory`'s bound, as the audit's calls are; a
/// failure writes its line.
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
    /// The refusal of an admitted request whose `user` (the token's `user_claim`, a string) the
    /// directory holds deactivated or deleted after it was provisioned: a 403 `deprovisioned`,
    /// whatever its role. A token without the claim, or a user the directory never held, is
    /// admitted. When the directory cannot be read, reads are admitted and a `write` is the 503
    /// `audit_unavailable`, since the console cannot tell whether the subject is deprovisioned;
    /// the audit insert is not attempted. The second item is the line the audit prints.
    #[cfg_attr(not(feature = "oidc"), allow(dead_code))]
    pub(super) fn directory_refusal(
        &self,
        user: Option<&str>,
        write: bool,
    ) -> Option<(Box<Answer>, String)> {
        let (directory, user) = (self.directory.as_ref()?, user?);
        match blocking(directory, directory.deprovisioned(user)) {
            Some(false) => None,
            Some(true) => {
                let message = Msg::UiDeprovisioned.text();
                let answer = error(StatusCode::FORBIDDEN, "deprovisioned", &message);
                Some((Box::new(answer), message))
            }
            None if write => {
                let message = Msg::AuditUnavailable.text();
                let status = StatusCode::SERVICE_UNAVAILABLE;
                Some((
                    Box::new(error(status, "audit_unavailable", &message)),
                    message,
                ))
            }
            None => None,
        }
    }

    /// `GET directory/groups?prefix=<text>` (global operator only, S2's role refusal for anyone
    /// else): `{"groups": [...]}`, at most `PICKER_MAX` values of `group_attribute` starting with the
    /// percent-decoded prefix (none lists the first `PICKER_MAX`), exactly compared, sorted by byte. More
    /// than one parameter, another name or a prefix over `OWNER_MAX` characters is a 400.
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
        // No stored name holds a NUL: a prefix with one lists nothing, and Postgres is not asked.
        if prefix.contains('\0') {
            return json_answer(StatusCode::OK, &json!({"groups": []}));
        }
        match blocking(directory, directory.groups(&prefix)) {
            Some(groups) => json_answer(StatusCode::OK, &json!({"groups": groups})),
            None => {
                let message = Msg::DirectoryUnavailable.text();
                error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message)
            }
        }
    }

    /// `owner_missing: true` on each of `entries` whose `owner` names no group of the directory,
    /// once a group was ever provisioned (a directory that lost its final group flags every
    /// owned entry). An entry without an owner is never flagged. A directory that cannot be read
    /// flags nothing, with one stderr line.
    #[cfg_attr(not(feature = "generation-pointer"), allow(dead_code))]
    pub(super) async fn flag_missing_owners(&self, entries: &mut [Value]) {
        let Some(directory) = &self.directory else {
            return;
        };
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

    /// `document`, the admission answer, with the directory's counts: `directory` is
    /// `{"users", "groups", "last_write"}`, or null when it cannot be read.
    #[cfg_attr(not(feature = "oidc"), allow(dead_code))]
    pub(super) fn with_directory_counts(&self, document: &mut Value) {
        let Some(directory) = &self.directory else {
            return;
        };
        document["directory"] = match blocking(directory, directory.counts()) {
            Some(counts) => json!({
                "users": counts.users,
                "groups": counts.groups,
                "last_write": counts.last_write,
            }),
            None => Value::Null,
        };
    }
}
