//! The version lifecycle of named workspaces (#343, ADR 0021 contract 4): save, apply and
//! rollback, rename, delete and the operator list, each one transaction on the workspace's row.
//!
//! This is the atomic boundary. `SELECT ... FOR UPDATE` on the row, then in this order: the
//! workspace is not deleted, `seq` is the `base` the caller read, the actor is an operator of it
//! or the platform administrator and, for an operation that adds or changes entries, the newest
//! permissions set (read here, not the admission cache) holds every one of those entries
//! eligible for the actor itself. Only then do `current` and `seq` move and the history row is
//! inserted. A stale page, a revoked grant or a refused entry returns before any write, so every
//! row stays as it was. Saving a version stores a row and moves nothing. The version document is
//! the shared store's, written by the caller before this runs; nothing here names a row that
//! points at a document that is not there.

use std::collections::BTreeSet;

use meada_core::authorize::Grant;
use meada_core::named::{self, Creation};

use super::store::{Creator, Gate, Policy, TIME, eligible};
use crate::msg::Msg;
use crate::postgres;

// ponytail: the most operators one workspace holds (ADR 0021), the most versions and history
// rows a page lists.
pub(crate) const OPERATORS_MAX: usize = 20;
pub(crate) const PAGE_MAX: usize = 50;
// ponytail: the longest subject a delegation names, in bytes.
pub(crate) const SUBJECT_MAX: usize = 256;

/// Whether an operators request is well formed: `add` pairs of subject and GitHub login and
/// `remove` subjects, at least one change and at most `2 * OPERATORS_MAX` of them, every subject
/// plain and within `SUBJECT_MAX`, every login one the sponsor rule can read, no subject twice in
/// either list and none in both. The console and the CLI decide it here.
pub(crate) fn operators_valid(add: &[(String, String)], remove: &[String]) -> bool {
    let subject_ok =
        |s: &str| !s.is_empty() && s.len() <= SUBJECT_MAX && !s.chars().any(char::is_control);
    let added: BTreeSet<&str> = add.iter().map(|(s, _)| s.as_str()).collect();
    let removed: BTreeSet<&str> = remove.iter().map(String::as_str).collect();
    let total = add.len() + remove.len();
    total > 0
        && total <= OPERATORS_MAX * 2
        && add
            .iter()
            .all(|(s, l)| subject_ok(s) && meada_core::manifest::added_by_problem(l).is_none())
        && remove.iter().all(|s| subject_ok(s))
        && added.len() == add.len()
        && removed.len() == remove.len()
        && added.intersection(&removed).next().is_none()
}

/// Why an operation did not happen. Nothing was written for any of them.
#[derive(Debug)]
pub(crate) enum Error {
    /// The workspace does not exist or was deleted.
    NotFound,
    /// `seq` is not the base the caller read: the current `seq`.
    Stale(i64),
    /// The actor neither operates the workspace nor is the platform administrator.
    NotOperator,
    /// The operation is the platform administrator's.
    NotAdministrator,
    /// A condition of the platform or the caller that stops every selection (fail closed).
    Pending(&'static str),
    /// Some entry the operation adds or changes is not eligible for the actor.
    Refused(Creation),
    /// The version is not one of this workspace's.
    UnknownVersion,
    /// The version applied is the one that is current.
    AlreadyCurrent,
    /// The operator list would hold none or more than `OPERATORS_MAX`: `last_operator` or
    /// `too_many_operators`.
    Operators(&'static str),
    /// A database error, scrubbed of the URL.
    Failed(String),
    Db(DbError),
}

/// A database error before it is scrubbed.
pub(crate) struct DbError(tokio_postgres::Error);

impl std::fmt::Debug for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DbError")
    }
}

fn db(e: tokio_postgres::Error) -> Error {
    Error::Db(DbError(e))
}

/// The fact an operation targets: the workspace, the `seq` the caller read and the version that
/// was current when the entries it carries were computed against it.
pub(crate) struct Target<'a> {
    pub id: &'a str,
    pub base: i64,
    pub current: &'a str,
}

/// What an operation changes.
pub(crate) enum Op<'a> {
    /// Stores a version row: `identities` are the version document's, `changed` the ones it adds
    /// or changes against the current version.
    Save {
        version: &'a str,
        message: &'a str,
        identities: &'a [String],
        changed: &'a [String],
    },
    /// Makes a stored version current.
    Apply {
        version: &'a str,
        identities: &'a [String],
        changed: &'a [String],
    },
    Rename {
        name: &'a str,
        description: &'a str,
    },
    Delete,
    /// The platform administrator's: operators to add (subject, login) and subjects to remove.
    Operators {
        add: &'a [(String, String)],
        remove: &'a [String],
    },
}

/// What an accepted operation did.
#[derive(Debug)]
pub(crate) struct Done {
    /// The workspace's `seq` after it: the next base.
    pub seq: i64,
    /// The history action, `save` for a stored version that moves nothing.
    pub action: &'static str,
    /// For a save: whether the row was new (a version already stored is not stored again).
    pub saved: bool,
}

fn authority(actor: &Creator<'_>) -> &'static str {
    match actor.administrator {
        true => "platform_administrator",
        false => "workspace_operator",
    }
}

/// Runs `op` on the workspace `target` as `actor` in one transaction. `grant` is what the actor
/// reads, to word a refusal.
pub(crate) async fn transact(
    policy: &Policy<'_>,
    actor: &Creator<'_>,
    grant: &Grant,
    target: &Target<'_>,
    op: &Op<'_>,
) -> Result<Done, Error> {
    let mut opened = postgres::connected(policy.postgres)
        .await
        .map_err(Error::Failed)?;
    let scrub = opened.scrubber();
    let q = policy.postgres.quoted();
    let scrubbed = |e| match e {
        Error::Db(DbError(e)) => Error::Failed(scrub(e)),
        other => other,
    };
    let tx = (opened.client.transaction().await).map_err(|e| Error::Failed(scrub(e)))?;
    let done = run(&tx, &q, policy, actor, grant, target, op)
        .await
        .map_err(scrubbed)?;
    tx.commit().await.map_err(|e| Error::Failed(scrub(e)))?;
    Ok(done)
}

async fn run(
    tx: &tokio_postgres::Transaction<'_>,
    q: &str,
    policy: &Policy<'_>,
    actor: &Creator<'_>,
    grant: &Grant,
    target: &Target<'_>,
    op: &Op<'_>,
) -> Result<Done, Error> {
    let id = target.id;
    let lock = format!(
        "SELECT seq, current FROM {q}.workspaces WHERE id = $1 AND deleted IS NULL FOR UPDATE"
    );
    let Some(row) = tx.query_opt(&lock, &[&id]).await.map_err(db)? else {
        return Err(Error::NotFound);
    };
    let (seq, current): (i64, String) = (row.get(0), row.get(1));
    if seq != target.base {
        return Err(Error::Stale(seq));
    }
    if matches!(op, Op::Operators { .. }) && !actor.administrator {
        return Err(Error::NotAdministrator);
    }
    if !actor.administrator {
        let operator = format!(
            "SELECT 1 FROM {q}.workspace_operators WHERE workspace_id = $1 AND subject = $2"
        );
        if (tx.query_opt(&operator, &[&id, &actor.subject]).await)
            .map_err(db)?
            .is_none()
        {
            return Err(Error::NotOperator);
        }
    }
    // The entries the operation carries were computed against another version than the one
    // that is current: the base cannot be the same fact.
    if current != target.current {
        return Err(Error::Stale(seq));
    }
    match op {
        Op::Save {
            version,
            message,
            identities,
            changed,
        } => {
            gate(tx, q, policy, actor, grant, changed).await?;
            let insert = format!(
                "INSERT INTO {q}.workspace_versions (workspace_id, version, author, message, \
                 entries) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING"
            );
            let params: [&(dyn tokio_postgres::types::ToSql + Sync); 5] =
                [&id, version, &actor.subject, message, identities];
            let stored = tx.execute(&insert, &params).await.map_err(db)?;
            Ok(Done {
                seq,
                action: "save",
                saved: stored == 1,
            })
        }
        Op::Apply {
            version,
            identities,
            changed,
        } => {
            let known = format!(
                "SELECT 1 FROM {q}.workspace_versions WHERE workspace_id = $1 AND version = $2"
            );
            if (tx.query_opt(&known, &[&id, version]).await)
                .map_err(db)?
                .is_none()
            {
                return Err(Error::UnknownVersion);
            }
            if *version == current {
                return Err(Error::AlreadyCurrent);
            }
            gate(tx, q, policy, actor, grant, changed).await?;
            let rollback = was_current_in(tx, q, id, version).await.map_err(db)?;
            let action = match rollback {
                true => "rollback",
                false => "apply",
            };
            let update = format!(
                "UPDATE {q}.workspaces SET current = $2, entries = $3, seq = seq + 1 \
                 WHERE id = $1"
            );
            tx.execute(&update, &[&id, version, identities])
                .await
                .map_err(db)?;
            history(tx, q, id, seq + 1, action, Some(version), actor, None).await?;
            Ok(Done {
                seq: seq + 1,
                action,
                saved: false,
            })
        }
        Op::Rename { name, description } => {
            let update = format!(
                "UPDATE {q}.workspaces SET name = $2, description = $3, seq = seq + 1 \
                 WHERE id = $1"
            );
            tx.execute(&update, &[&id, name, description])
                .await
                .map_err(db)?;
            history(tx, q, id, seq + 1, "rename", None, actor, None).await?;
            Ok(Done {
                seq: seq + 1,
                action: "rename",
                saved: false,
            })
        }
        Op::Delete => {
            let update =
                format!("UPDATE {q}.workspaces SET deleted = now(), seq = seq + 1 WHERE id = $1");
            tx.execute(&update, &[&id]).await.map_err(db)?;
            history(tx, q, id, seq + 1, "delete", None, actor, None).await?;
            Ok(Done {
                seq: seq + 1,
                action: "delete",
                saved: false,
            })
        }
        Op::Operators { add, remove } => {
            let drop = format!(
                "DELETE FROM {q}.workspace_operators WHERE workspace_id = $1 AND subject = ANY($2)"
            );
            tx.execute(&drop, &[&id, remove]).await.map_err(db)?;
            let upsert = format!(
                "INSERT INTO {q}.workspace_operators (workspace_id, subject, login, added_by) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (workspace_id, subject) DO UPDATE SET login = EXCLUDED.login"
            );
            for (subject, login) in *add {
                let params: [&(dyn tokio_postgres::types::ToSql + Sync); 4] =
                    [&id, subject, login, &actor.subject];
                tx.execute(&upsert, &params).await.map_err(db)?;
            }
            let count =
                format!("SELECT count(*) FROM {q}.workspace_operators WHERE workspace_id = $1");
            let held: i64 = tx.query_one(&count, &[&id]).await.map_err(db)?.get(0);
            if held == 0 {
                return Err(Error::Operators("last_operator"));
            }
            if held > OPERATORS_MAX as i64 {
                return Err(Error::Operators("too_many_operators"));
            }
            let note = format!("added {}, removed {}", add.len(), remove.len());
            history(tx, q, id, seq + 1, "operators", None, actor, Some(&note)).await?;
            let bump = format!("UPDATE {q}.workspaces SET seq = seq + 1 WHERE id = $1");
            tx.execute(&bump, &[&id]).await.map_err(db)?;
            Ok(Done {
                seq: seq + 1,
                action: "operators",
                saved: false,
            })
        }
    }
}

/// The eligibility of the entries an operation adds or changes, for the actor itself, from the
/// newest set in `tx`. Entries both versions hold and removals need none; a refusal names the
/// identities `grant` reads and counts the others.
async fn gate(
    tx: &tokio_postgres::Transaction<'_>,
    q: &str,
    policy: &Policy<'_>,
    actor: &Creator<'_>,
    grant: &Grant,
    changed: &[String],
) -> Result<(), Error> {
    if changed.is_empty() {
        return Ok(());
    }
    let allowed: BTreeSet<String> = match eligible(tx, q, policy, actor, changed).await {
        Ok(allowed) => allowed,
        Err(Gate::Pending(reason)) => return Err(Error::Pending(reason)),
        Err(Gate::Db(e)) => return Err(db(e)),
    };
    let decided = named::creation(changed, &allowed, grant);
    match decided.allowed {
        true => Ok(()),
        false => Err(Error::Refused(decided)),
    }
}

#[allow(clippy::too_many_arguments)]
async fn history(
    tx: &tokio_postgres::Transaction<'_>,
    q: &str,
    id: &str,
    seq: i64,
    action: &str,
    version: Option<&str>,
    actor: &Creator<'_>,
    message: Option<&str>,
) -> Result<(), Error> {
    let insert = format!(
        "INSERT INTO {q}.workspace_history (workspace_id, seq, action, version, actor, \
         authority, message) VALUES ($1, $2, $3, $4, $5, $6, $7)"
    );
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 7] = [
        &id,
        &seq,
        &action,
        &version,
        &actor.subject,
        &authority(actor),
        &message,
    ];
    tx.execute(&insert, &params).await.map_err(db)?;
    Ok(())
}

/// Whether `version` was ever the workspace's current one: an apply of it is a rollback.
async fn was_current_in(
    tx: &tokio_postgres::Transaction<'_>,
    q: &str,
    id: &str,
    version: &str,
) -> Result<bool, tokio_postgres::Error> {
    let sql = format!(
        "SELECT EXISTS (SELECT 1 FROM {q}.workspace_history WHERE workspace_id = $1 \
         AND version = $2 AND action IN ('create', 'apply', 'rollback'))"
    );
    Ok(tx.query_one(&sql, &[&id, &version]).await?.get(0))
}

/// Whether applying `version` of the workspace is a rollback (the audit names the action before
/// the transaction runs; the history is append only, so the answer holds).
pub(crate) async fn was_current(
    postgres: &postgres::Config,
    id: &str,
    version: &str,
) -> Result<bool, String> {
    let opened = postgres::connected(postgres).await?;
    let q = postgres.quoted();
    let sql = format!(
        "SELECT EXISTS (SELECT 1 FROM {q}.workspace_history WHERE workspace_id = $1 \
         AND version = $2 AND action IN ('create', 'apply', 'rollback'))"
    );
    let row =
        (opened.client.query_one(&sql, &[&id, &version]).await).map_err(|e| opened.scrub(e))?;
    Ok(row.get(0))
}

/// One version of a workspace as a list shows it.
pub(crate) struct VersionRow {
    pub version: String,
    pub author: String,
    pub saved: String,
    pub message: String,
    /// The identities of its document, in manifest order.
    pub entries: Vec<String>,
    /// Whether it is the workspace's current version.
    pub current: bool,
    /// Where the next page of a list starts after this row.
    pub cursor: String,
}

/// The sort key of a version row and of the opaque cursor: its time at UTC to the microsecond,
/// 20 digits, and then its id.
const SAVED_KEY: &str = "to_char(v.saved AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS')";

/// The shape of a cursor: 20 digits and a version id.
pub(crate) fn cursor_ok(after: &str) -> bool {
    after.len() == 36
        && after[..20].bytes().all(|b| b.is_ascii_digit())
        && meada_core::ids::is_id(&after[20..])
}

fn version_row(row: &tokio_postgres::Row, bad: &dyn Fn() -> String) -> Result<VersionRow, String> {
    let version: String = row.try_get(0).map_err(|_| bad())?;
    if !meada_core::ids::is_id(&version) {
        return Err(bad());
    }
    let key: String = row.try_get(6).map_err(|_| bad())?;
    Ok(VersionRow {
        author: row.try_get(1).map_err(|_| bad())?,
        saved: row.try_get(2).map_err(|_| bad())?,
        message: row.try_get(3).map_err(|_| bad())?,
        entries: row.try_get(4).map_err(|_| bad())?,
        current: row.try_get(5).map_err(|_| bad())?,
        cursor: format!("{key}{version}"),
        version,
    })
}

const VERSION_COLUMNS: &str = "v.version, v.author, to_char(v.saved AT TIME ZONE 'UTC', \
     'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), v.message, v.entries, (v.version = w.current)";

/// The versions of workspace `id`, newest first, after the cursor `after`, at most `limit`
/// rows. `only` narrows to the versions whose every identity it holds (a reader that is no
/// operator sees the versions it reads whole); `None` lists them all.
pub(crate) async fn versions(
    postgres: &postgres::Config,
    id: &str,
    only: Option<&BTreeSet<String>>,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<VersionRow>, String> {
    let opened = postgres::connected(postgres).await?;
    let q = postgres.quoted();
    let sql = format!(
        "SELECT {VERSION_COLUMNS}, {SAVED_KEY} FROM {q}.workspace_versions v \
         JOIN {q}.workspaces w ON w.id = v.workspace_id \
         WHERE v.workspace_id = $1 AND ($2::text[] IS NULL OR v.entries <@ $2) \
         AND ($3::text IS NULL OR ({SAVED_KEY}, v.version) < ($3::text, $4::text)) \
         ORDER BY {SAVED_KEY} DESC, v.version DESC LIMIT $5"
    );
    let only: Option<Vec<String>> = only.map(|s| s.iter().cloned().collect());
    let (saved, version) = match after {
        Some(a) => (Some(a[..20].to_string()), Some(a[20..].to_string())),
        None => (None, None),
    };
    let limit = limit as i64;
    let rows = (opened.client)
        .query(&sql, &[&id, &only, &saved, &version, &limit])
        .await
        .map_err(|e| opened.scrub(e))?;
    let bad = || Msg::PermissionsBadRow(&format!("{q}.workspace_versions")).text();
    rows.iter().map(|row| version_row(row, &bad)).collect()
}

/// One version of workspace `id`, or none when the workspace holds no such row.
pub(crate) async fn version(
    postgres: &postgres::Config,
    id: &str,
    version: &str,
) -> Result<Option<VersionRow>, String> {
    let opened = postgres::connected(postgres).await?;
    let q = postgres.quoted();
    let sql = format!(
        "SELECT {VERSION_COLUMNS}, {SAVED_KEY} FROM {q}.workspace_versions v \
         JOIN {q}.workspaces w ON w.id = v.workspace_id \
         WHERE v.workspace_id = $1 AND v.version = $2"
    );
    let rows = (opened.client.query(&sql, &[&id, &version]).await).map_err(|e| opened.scrub(e))?;
    let bad = || Msg::PermissionsBadRow(&format!("{q}.workspace_versions")).text();
    rows.first().map(|row| version_row(row, &bad)).transpose()
}

/// One row of a workspace's append-only history.
pub(crate) struct Entry {
    pub seq: i64,
    pub action: String,
    pub version: Option<String>,
    pub actor: String,
    pub authority: String,
    pub time: String,
    pub message: Option<String>,
}

/// The history of workspace `id`, newest first, the rows with a `seq` below `before`, at most
/// `limit`.
pub(crate) async fn histories(
    postgres: &postgres::Config,
    id: &str,
    before: Option<i64>,
    limit: usize,
) -> Result<Vec<Entry>, String> {
    let opened = postgres::connected(postgres).await?;
    let q = postgres.quoted();
    let sql = format!(
        "SELECT seq, action, version, actor, authority, \
         to_char(\"time\" AT TIME ZONE 'UTC', {TIME}), message \
         FROM {q}.workspace_history WHERE workspace_id = $1 AND seq < $2 \
         ORDER BY seq DESC LIMIT $3"
    );
    let before = before.unwrap_or(i64::MAX);
    let limit = limit as i64;
    let rows = (opened.client)
        .query(&sql, &[&id, &before, &limit])
        .await
        .map_err(|e| opened.scrub(e))?;
    let bad = || Msg::PermissionsBadRow(&format!("{q}.workspace_history")).text();
    rows.iter()
        .map(|row| {
            Ok(Entry {
                seq: row.try_get(0).map_err(|_| bad())?,
                action: row.try_get(1).map_err(|_| bad())?,
                version: row.try_get(2).map_err(|_| bad())?,
                actor: row.try_get(3).map_err(|_| bad())?,
                authority: row.try_get(4).map_err(|_| bad())?,
                time: row.try_get(5).map_err(|_| bad())?,
                message: row.try_get(6).map_err(|_| bad())?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(subject: &str) -> (String, String) {
        (subject.into(), "dave".into())
    }

    #[test]
    fn an_operators_request_is_bounded_and_plain() {
        assert!(operators_valid(&[add("a")], &["b".into()]));
        assert!(!operators_valid(&[], &[]), "no change");
        assert!(
            !operators_valid(&[add("a"), add("a")], &[]),
            "a subject twice"
        );
        assert!(
            !operators_valid(&[add("a")], &["a".into()]),
            "in both lists"
        );
        assert!(!operators_valid(&[add("")], &[]));
        assert!(!operators_valid(&[add("a\nb")], &[]));
        assert!(!operators_valid(&[("a".into(), "not a login".into())], &[]));
        // Past the longest subject, and past the most changes one request carries.
        let long = "x".repeat(SUBJECT_MAX + 1);
        assert!(operators_valid(&[add(&"x".repeat(SUBJECT_MAX))], &[]));
        assert!(!operators_valid(&[add(&long)], &[]));
        assert!(!operators_valid(&[], &[long]));
        let many: Vec<_> = (0..=OPERATORS_MAX * 2)
            .map(|i| add(&format!("s{i}")))
            .collect();
        assert!(!operators_valid(&many, &[]));
        assert!(operators_valid(&many[..OPERATORS_MAX * 2], &[]));
    }

    #[test]
    fn a_cursor_is_twenty_digits_and_a_version_id() {
        let id = "0123456789abcdef";
        assert!(cursor_ok(&format!("{}{id}", "1".repeat(20))));
        for bad in [
            id.to_string(),
            format!("{}{id}", "1".repeat(19)),
            format!("{}{id}", "x".repeat(20)),
            format!("{}{}", "1".repeat(20), "G".repeat(16)),
            format!("{}{id}0", "1".repeat(20)),
        ] {
            assert!(!cursor_ok(&bad), "{bad}");
        }
    }
}
