//! The records of named workspaces in Postgres (ADR 0021 contract 4, #342): four tables beside
//! the permissions set, and the one transaction that creates a workspace. Creation is the atomic
//! boundary of ADR 0021: in one transaction it reads the newest permissions set (not the
//! admission cache), decides eligibility for every selected repository from it, and only then
//! inserts the workspace, its first operator, its first version and its history. Nothing is
//! stored when a repository is refused, so a revoked grant or a forged payload authorizes
//! nothing. The version document itself is the shared store's, immutable and content-addressed
//! (`document`); this file holds no author, time or message of it.

use std::collections::BTreeSet;
use std::time::Duration;

use meada_core::authorize::Grant;
use meada_core::named::{self, Creation};
use serde_json::{Value as Json, json};

use crate::msg::Msg;
use crate::postgres::{self, Opened};

/// The key of the advisory lock that serializes workspace creation, so that `max_workspaces`
/// holds under concurrent creates.
const CREATE_LOCK: i64 = 0x6d65_6164_6177_6b73;

/// What decides eligibility: the section's settings and `[oidc.github]`'s accepted roles and the
/// age past which a set grants nothing.
pub(crate) struct Policy<'a> {
    pub postgres: &'a postgres::Config,
    pub max_age: Duration,
    pub roles: &'a [String],
    pub max_workspaces: usize,
}

/// Who creates: the authenticated subject, its GitHub login and whether it is the platform
/// administrator. Nothing here comes from the request body.
pub(crate) struct Creator<'a> {
    pub subject: &'a str,
    pub login: Option<&'a str>,
    pub administrator: bool,
}

/// A workspace to create.
pub(crate) struct New<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub version: &'a str,
    pub entries: &'a [Json],
    pub message: &'a str,
}

/// Why a creation did not happen.
#[derive(Debug)]
pub(crate) enum CreateError {
    /// A condition of the platform or the caller, the same for every identity: `no_set`,
    /// `stale_set` or `missing_login`. Nothing can be selected (fail closed).
    Pending(&'static str),
    /// Some selected identity is not eligible: the ones the grant reads, named, and the count of
    /// the others.
    Refused(Creation),
    /// `max_workspaces` workspaces exist.
    Limit,
    /// The same triple was created and then deleted.
    Deleted,
    Failed(String),
}

/// Why the permissions set cannot decide eligibility.
pub(super) enum Gate {
    /// A condition of the platform or the caller, the same for every identity: `no_set`,
    /// `stale_set` or `missing_login`. Nothing can be selected (fail closed).
    Pending(&'static str),
    Db(tokio_postgres::Error),
}

/// The identities of `selected` that `actor` may select, decided in `tx` from the newest
/// permissions set on Postgres's clock (not the admission cache): the catalog holds the
/// identity, and a login holds an accepted role on it; for the platform administrator, the
/// catalog alone. Pending when no set exists, the set is older than `max_age` or a subject
/// other than the administrator has no login.
pub(super) async fn eligible(
    tx: &tokio_postgres::Transaction<'_>,
    q: &str,
    policy: &Policy<'_>,
    actor: &Creator<'_>,
    selected: &[String],
) -> Result<BTreeSet<String>, Gate> {
    let newest = format!(
        "SELECT id, extract(epoch FROM clock_timestamp() - finished)::float8 \
         FROM {q}.permission_sets ORDER BY fence DESC, finished DESC LIMIT 1"
    );
    let Some(row) = tx.query_opt(&newest, &[]).await.map_err(Gate::Db)? else {
        return Err(Gate::Pending("no_set"));
    };
    let (set, age): (String, f64) = (row.get(0), row.get(1));
    if !age.is_finite() || age < 0.0 || age > policy.max_age.as_secs_f64() {
        return Err(Gate::Pending("stale_set"));
    }
    let login = actor.login;
    if !actor.administrator && login.is_none() {
        return Err(Gate::Pending("missing_login"));
    }
    // The catalog holds the identity; a login is eligible when it holds an accepted role on it.
    let catalog = format!(
        "SELECT p.identity FROM {q}.permission_repositories p \
         WHERE p.set_id = $1 AND p.coverage = 'listed' AND p.source IN ('catalog', 'both') \
         AND p.identity = ANY($2) AND ($3::text IS NULL OR EXISTS ( \
           SELECT 1 FROM {q}.permission_rows r WHERE r.set_id = p.set_id \
           AND r.identity = p.identity AND r.login = $3 AND r.role = ANY($4)))"
    );
    let only = (!actor.administrator).then_some(login).flatten();
    let rows = tx
        .query(&catalog, &[&set, &selected, &only, &policy.roles])
        .await
        .map_err(Gate::Db)?;
    Ok(rows.iter().map(|r| r.get(0)).collect())
}

/// A created workspace, or the one the same creator and entries already made.
pub(crate) struct Created {
    pub id: String,
    pub created: bool,
}

/// `to_char` for a time column at UTC, to the second.
pub(super) const TIME: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'";

/// Creates the workspace `new` for `creator` (ADR 0021): the checks and the inserts in one
/// transaction under the creation lock. `grant` is what the caller reads, to word a refusal.
pub(crate) async fn create(
    policy: &Policy<'_>,
    creator: &Creator<'_>,
    grant: &Grant,
    new: &New<'_>,
) -> Result<Created, CreateError> {
    let failed = CreateError::Failed;
    let mut opened: Opened = postgres::connected(policy.postgres).await.map_err(failed)?;
    let scrub = opened.scrubber();
    let failed = |e| CreateError::Failed(scrub(e));
    let q = policy.postgres.quoted();
    let tx = opened.client.transaction().await.map_err(failed)?;
    tx.execute("SELECT pg_advisory_xact_lock($1)", &[&CREATE_LOCK])
        .await
        .map_err(failed)?;
    let login = creator.login;
    let selected = named::identities(new.entries);
    let eligible = match eligible(&tx, &q, policy, creator, &selected).await {
        Ok(eligible) => eligible,
        Err(Gate::Pending(reason)) => return Err(CreateError::Pending(reason)),
        Err(Gate::Db(e)) => return Err(failed(e)),
    };
    let decided = named::creation(&selected, &eligible, grant);
    if !decided.allowed {
        return Err(CreateError::Refused(decided));
    }
    let id = meada_core::ids::workspace_id(creator.subject, new.name, new.version);
    let existing = format!("SELECT deleted IS NOT NULL FROM {q}.workspaces WHERE id = $1");
    if let Some(row) = tx.query_opt(&existing, &[&id]).await.map_err(failed)? {
        return match row.get::<_, bool>(0) {
            true => Err(CreateError::Deleted),
            false => Ok(Created { id, created: false }),
        };
    }
    let count = format!("SELECT count(*) FROM {q}.workspaces WHERE deleted IS NULL");
    let held: i64 = tx.query_one(&count, &[]).await.map_err(failed)?.get(0);
    if held >= policy.max_workspaces as i64 {
        return Err(CreateError::Limit);
    }
    let authority = match creator.administrator {
        true => "platform_administrator",
        false => "workspace_operator",
    };
    let insert = format!(
        "INSERT INTO {q}.workspaces (id, name, description, creator, creator_login, seq, \
         current, entries) VALUES ($1, $2, $3, $4, $5, 1, $6, $7)"
    );
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 7] = [
        &id,
        &new.name,
        &new.description,
        &creator.subject,
        &login,
        &new.version,
        &selected,
    ];
    tx.execute(&insert, &params).await.map_err(failed)?;
    let operator = format!(
        "INSERT INTO {q}.workspace_operators (workspace_id, subject, login, added_by) \
         VALUES ($1, $2, $3, $2)"
    );
    tx.execute(&operator, &[&id, &creator.subject, &login])
        .await
        .map_err(failed)?;
    let version = format!(
        "INSERT INTO {q}.workspace_versions (workspace_id, version, author, message, entries) \
         VALUES ($1, $2, $3, $4, $5)"
    );
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] =
        [&id, &new.version, &creator.subject, &new.message, &selected];
    tx.execute(&version, &params).await.map_err(failed)?;
    let history = format!(
        "INSERT INTO {q}.workspace_history (workspace_id, seq, action, version, actor, \
         authority, message) VALUES ($1, 1, 'create', $2, $3, $4, $5)"
    );
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] = [
        &id,
        &new.version,
        &creator.subject,
        &authority,
        &new.message,
    ];
    tx.execute(&history, &params).await.map_err(failed)?;
    tx.commit().await.map_err(failed)?;
    Ok(Created { id, created: true })
}

/// One workspace as the console lists and shows it.
pub(crate) struct Record {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created: String,
    pub seq: i64,
    pub current: String,
    /// The identities of the current version, in manifest order.
    pub entries: Vec<String>,
    /// Each operator: its subject and its GitHub login.
    pub operators: Vec<(String, Option<String>)>,
}

/// Every workspace not deleted, ordered by id, with its operators; at most `max` rows. `only`
/// narrows to one id.
pub(crate) async fn records(
    postgres: &postgres::Config,
    only: Option<&str>,
    max: usize,
) -> Result<Vec<Record>, String> {
    let opened = postgres::connected(postgres).await?;
    let q = postgres.quoted();
    let sql = format!(
        "SELECT w.id, w.name, w.description, \
         to_char(w.created AT TIME ZONE 'UTC', {TIME}), w.seq, w.current, w.entries, \
         COALESCE((SELECT jsonb_agg(jsonb_build_array(o.subject, o.login) \
                   ORDER BY o.added, o.subject) \
                   FROM {q}.workspace_operators o WHERE o.workspace_id = w.id), '[]'::jsonb)::text \
         FROM {q}.workspaces w WHERE w.deleted IS NULL AND ($1::text IS NULL OR w.id = $1) \
         ORDER BY w.id LIMIT $2"
    );
    let limit = max as i64;
    let rows = (opened.client.query(&sql, &[&only, &limit]).await).map_err(|e| opened.scrub(e))?;
    let mut records = Vec::new();
    for row in rows {
        let operators: Json =
            serde_json::from_str(&row.get::<_, String>(7)).map_err(|e| e.to_string())?;
        let operators = (operators.as_array().into_iter().flatten())
            .filter_map(|o| Some((o[0].as_str()?.to_string(), o[1].as_str().map(String::from))))
            .collect();
        // Every row is hostile: one that is not what the migration wrote is an error naming the
        // table, never a panic or a value the pages would show.
        let bad = || Msg::PermissionsBadRow(&format!("{q}.workspaces")).text();
        let (id, current): (String, String) = (
            row.try_get(0).map_err(|_| bad())?,
            row.try_get(5).map_err(|_| bad())?,
        );
        if !meada_core::ids::is_id(&id) || !meada_core::ids::is_id(&current) {
            return Err(bad());
        }
        records.push(Record {
            id,
            name: row.try_get(1).map_err(|_| bad())?,
            description: row.try_get(2).map_err(|_| bad())?,
            created: row.try_get(3).map_err(|_| bad())?,
            seq: row.try_get(4).map_err(|_| bad())?,
            current,
            entries: row.try_get(6).map_err(|_| bad())?,
            operators,
        });
    }
    Ok(records)
}

/// What `meada workspace pull` reads: every workspace not deleted, its id, current version and
/// the logins of its operators, in one `REPEATABLE READ` transaction, ordered by id.
pub(crate) async fn current_contexts(
    postgres: &postgres::Config,
) -> Result<Vec<(String, String, Vec<String>)>, String> {
    let mut opened = postgres::connected(postgres).await?;
    let scrub = opened.scrubber();
    let q = postgres.quoted();
    let tx = (opened.client.build_transaction())
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(&scrub)?;
    let sql = format!(
        "SELECT w.id, w.current, \
         COALESCE((SELECT array_agg(o.login ORDER BY o.added, o.subject) \
                   FROM {q}.workspace_operators o \
                   WHERE o.workspace_id = w.id AND o.login IS NOT NULL), '{{}}') \
         FROM {q}.workspaces w WHERE w.deleted IS NULL ORDER BY w.id"
    );
    let rows = tx.query(&sql, &[]).await.map_err(&scrub)?;
    let bad = || Msg::PermissionsBadRow(&format!("{q}.workspaces")).text();
    let mut read = Vec::new();
    for row in &rows {
        let operators: Vec<String> = row.try_get(2).map_err(|_| bad())?;
        let login_ok = |l: &String| meada_core::manifest::added_by_problem(l).is_none();
        if !operators.iter().all(login_ok) {
            return Err(bad());
        }
        read.push((
            row.try_get(0).map_err(|_| bad())?,
            row.try_get(1).map_err(|_| bad())?,
            operators,
        ));
    }
    tx.commit().await.map_err(&scrub)?;
    Ok(read)
}

/// A named version document in the shared store (`workspaces/<version id>.json`): its id and its
/// entries only, so one workspace never reads another's author, time or message through it.
pub(crate) fn document_text(version: &str, entries: &[Json]) -> Vec<u8> {
    let mut text = serde_json::to_vec_pretty(&json!({"id": version, "repositories": entries}))
        .unwrap_or_default();
    text.push(b'\n');
    text
}

/// The entries of the named version document `bytes`, hostile input: the id must be `version`,
/// the entries a valid named text of at most `ENTRIES_MAX` entries whose id is `version`.
pub(crate) fn document_entries(
    key: &str,
    version: &str,
    bytes: &[u8],
) -> Result<Vec<Json>, String> {
    let max = super::ENTRIES_MAX as usize;
    let bad = || crate::msg::Msg::WorkspacesDocumentBad(key).text();
    let document: Json = serde_json::from_slice(bytes).map_err(|_| bad())?;
    if document["id"].as_str() != Some(version) {
        return Err(bad());
    }
    let entries = document["repositories"].as_array().ok_or_else(bad)?;
    let text = meada_core::workspace::entries_text(entries).ok_or_else(bad)?;
    let checked = named::version_entries(key, &text, max).map_err(|_| bad())?;
    match named::version_id(&checked) == version {
        true => Ok(checked),
        false => Err(bad()),
    }
}

/// Stores the version document `entries` is in the shared store, unless it holds it already:
/// content-addressed and immutable, so a second writer of the same entries changes nothing.
pub(crate) fn put_document(
    snapshot: &crate::snapshot::Config,
    version: &str,
    entries: &[Json],
) -> Result<(), String> {
    let key = crate::snapshot::versions::version_key(snapshot, version);
    let bytes = document_text(version, entries);
    let put = async {
        let store = crate::snapshot::store::Store::connect(snapshot)?;
        store.create(&key, bytes).await.map(drop)
    };
    crate::modules::block_on(put).map_err(|e| e.to_string())??;
    Ok(())
}

/// The entries of the named version document `version` from the shared store, checked.
pub(crate) fn get_document(
    snapshot: &crate::snapshot::Config,
    version: &str,
) -> Result<Vec<Json>, String> {
    let key = crate::snapshot::versions::version_key(snapshot, version);
    let get = async {
        let store = crate::snapshot::store::Store::connect(snapshot)?;
        store
            .get(&key, crate::snapshot::versions::VERSION_MAX)
            .await
    };
    let bytes = crate::modules::block_on(get).map_err(|e| e.to_string())??;
    let bytes = bytes.ok_or_else(|| Msg::WorkspaceNoVersion(&key).text())?;
    document_entries(&key, version, &bytes)
}
