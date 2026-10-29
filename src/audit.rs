//! The audit (ADR 0016, #328): with `[postgres]`, every console write and every CLI twin of one
//! leaves a record in `<schema>.audit`: `id` (a sequence), `time` (Postgres's clock, RFC 3339,
//! UTC, microseconds), `via` (`console` or `cli`), `subject`, `login` (the session's GitHub login
//! under `[oidc.github]`, #332, else null), `role`, `owners` (a team operator's owner groups, as its stderr line names them),
//! `action` (`ACTIONS`), `arguments` (the operation body's names, `{}` for a refusal before the
//! body is read), `outcome` (`pending`, `accepted`, `refused` or `failed`) and `message`.
//!
//! It fails closed: a write that passed its authorization inserts its record as `pending`
//! (`Audit::pending`) before it touches Valkey or the store and refuses itself when the insert
//! fails; once done it sets the outcome (`Audit::finish`), and a failed update leaves the record
//! `pending`. A role refusal found before the body is read is inserted as `refused`
//! (`Audit::refused`), and one that finds Postgres unreachable leaves no record. A body that fails
//! its checks (400, 413, 422) is never recorded, nor are the `Host`, `Origin` and admission
//! refusals. Records are never deleted by Meada.
//! Without `[postgres]` (or the `postgres` feature) every call does nothing. Each call opens its
//! own connection through `postgres::connected`, so a schema version this binary does not know
//! counts as unreachable. No record holds a token, a raw body or the Postgres URL.

use std::path::Path;

use serde_json::Value;
#[cfg(feature = "postgres")]
use serde_json::json;

use crate::msg::Msg;

/// The actions a record names: the operations, and `pointer`, the route-level action of a
/// pointer request refused before its body is read.
pub const ACTIONS: [&str; 8] = [
    "workspace.save",
    "workspace.import",
    "workspace.apply",
    "pointer.set",
    "pointer.retain",
    "pointer.drop",
    "fence.recover",
    "pointer",
];

/// The outcome a finished write records.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Accepted,
    /// A role refusal, a held lock, an operation's own refusal, entries refused.
    Refused,
    /// An error of Valkey, the store or anything else.
    Failed,
}

impl Outcome {
    /// `Refused` when `refused`, else `Failed`: a write that did not succeed.
    pub fn of(refused: bool) -> Self {
        match refused {
            true => Outcome::Refused,
            false => Outcome::Failed,
        }
    }

    #[cfg_attr(not(feature = "postgres"), allow(dead_code))]
    fn name(self) -> &'static str {
        match self {
            Outcome::Accepted => "accepted",
            Outcome::Refused => "refused",
            Outcome::Failed => "failed",
        }
    }
}

/// Who acts and through what: `via` is `console` or `cli`, `role` `viewer`, `operator` or
/// `team_operator`, `owners` a team operator's owner groups.
pub struct Actor<'a> {
    pub via: &'static str,
    pub subject: Option<&'a str>,
    /// The subject's GitHub login, when its session has one (#332).
    pub login: Option<&'a str>,
    pub role: &'static str,
    pub owners: Option<&'a [String]>,
}

/// A process's audit: `[postgres]` when the manifest has it.
#[derive(Clone, Default)]
pub struct Audit {
    #[cfg(feature = "postgres")]
    config: Option<crate::postgres::Config>,
}

/// A record inserted as `pending`: its id, none without `[postgres]`.
pub struct Pending(#[cfg_attr(not(feature = "postgres"), allow(dead_code))] Option<i64>);

/// A page of records, newest first, and the `before` of the following one.
#[cfg_attr(not(feature = "postgres"), allow(dead_code))]
pub struct Page {
    pub records: Vec<Value>,
    pub next: Option<i64>,
    /// The rows that failed their checks, left out.
    pub skipped: usize,
}

impl Audit {
    #[cfg(feature = "postgres")]
    pub fn new(config: Option<crate::postgres::Config>) -> Self {
        Self { config }
    }

    /// The audit of the bootstrap manifest at `manifest`.
    pub fn of(manifest: &Path) -> crate::Result<Self> {
        #[cfg(feature = "postgres")]
        return Ok(Self::new(crate::workspace::Manifest::postgres(manifest)?));
        #[cfg(not(feature = "postgres"))]
        {
            let _ = manifest;
            Ok(Self::default())
        }
    }

    /// Whether writes are audited: the manifest has `[postgres]`.
    pub fn on(&self) -> bool {
        #[cfg(feature = "postgres")]
        return self.config.is_some();
        #[cfg(not(feature = "postgres"))]
        false
    }

    /// Inserts the record of a write about to run as `pending`; the reason Postgres cannot be
    /// reached (the version refusal included) when the insert fails, and then nothing may be
    /// written.
    pub fn pending(
        &self,
        actor: &Actor,
        action: &str,
        arguments: &Value,
    ) -> Result<Pending, String> {
        #[cfg(feature = "postgres")]
        if let Some(config) = &self.config {
            return insert(config, actor, action, arguments, "pending", None)
                .map(Some)
                .map(Pending);
        }
        let _ = (actor, action, arguments);
        Ok(Pending(None))
    }

    /// Inserts a refusal found before the write; one that finds Postgres unreachable leaves no
    /// record.
    pub fn refused(&self, actor: &Actor, action: &str, arguments: &Value, message: &str) {
        #[cfg(feature = "postgres")]
        if let Some(config) = &self.config {
            let _ = insert(config, actor, action, arguments, "refused", Some(message));
        }
        let _ = (actor, action, arguments, message);
    }

    /// Sets the outcome of `pending`, its message and the owners its stderr line names; a
    /// failed update leaves the record `pending`.
    pub fn finish(
        &self,
        pending: Pending,
        outcome: Outcome,
        message: &str,
        owners: Option<&[String]>,
    ) {
        #[cfg(feature = "postgres")]
        if let (Some(config), Some(id)) = (&self.config, pending.0) {
            let owners = owners.map(|o| json!(o).to_string());
            let update = async {
                let opened = crate::postgres::connected(config).await?;
                let sql = format!(
                    "UPDATE {}.audit SET outcome = $2, message = $3, owners = $4::text::jsonb \
                     WHERE id = $1 AND outcome = 'pending'",
                    config.quoted()
                );
                let done = (opened.client)
                    .execute(&sql, &[&id, &outcome.name(), &message, &owners])
                    .await;
                done.map(|_| ()).map_err(|e| opened.scrub(e))
            };
            let _ = crate::modules::block_on(update);
        }
        let _ = (pending, outcome, message, owners);
    }

    /// The records with an id below `before` (all without it), newest first, at most `limit`.
    /// Every row is hostile: one that fails its checks is left out and counted in `skipped`.
    #[cfg_attr(not(feature = "postgres"), allow(dead_code))]
    pub fn page(&self, before: Option<i64>, limit: i64) -> Result<Page, String> {
        #[cfg(feature = "postgres")]
        if let Some(config) = &self.config {
            return page(config, before, limit);
        }
        let _ = (before, limit);
        Ok(Page {
            records: Vec::new(),
            next: None,
            skipped: 0,
        })
    }

    /// The table the records live in, as a stderr line names it.
    #[cfg_attr(not(feature = "postgres"), allow(dead_code))]
    pub fn table(&self) -> String {
        #[cfg(feature = "postgres")]
        if let Some(config) = &self.config {
            return format!("{}.audit", config.quoted());
        }
        "audit".into()
    }
}

/// Inserts one record; its id.
#[cfg(feature = "postgres")]
fn insert(
    config: &crate::postgres::Config,
    actor: &Actor,
    action: &str,
    arguments: &Value,
    outcome: &str,
    message: Option<&str>,
) -> Result<i64, String> {
    let owners = actor.owners.map(|o| json!(o).to_string());
    let arguments = arguments.to_string();
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        let sql = format!(
            "INSERT INTO {}.audit (via, subject, login, role, owners, action, arguments, \
             outcome, message) VALUES ($1, $2, $3, $4, $5::text::jsonb, $6, $7::text::jsonb, \
             $8, $9) RETURNING id",
            config.quoted()
        );
        let row = (opened.client)
            .query_one(
                &sql,
                &[
                    &actor.via,
                    &actor.subject,
                    &actor.login,
                    &actor.role,
                    &owners,
                    &action,
                    &arguments,
                    &outcome,
                    &message,
                ],
            )
            .await;
        Ok(row.map_err(|e| opened.scrub(e))?.get::<_, i64>(0))
    };
    crate::modules::block_on(run).map_err(|e| e.to_string())?
}

#[cfg(feature = "postgres")]
fn page(config: &crate::postgres::Config, before: Option<i64>, limit: i64) -> Result<Page, String> {
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        // One row past the page tells whether another follows.
        let sql = format!(
            "SELECT id, to_char(\"time\" AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
             via, subject, login, role, owners::text, action, arguments::text, outcome, message \
             FROM {}.audit WHERE id < $1 ORDER BY id DESC LIMIT $2",
            config.quoted()
        );
        let before = before.unwrap_or(i64::MAX);
        let rows = (opened.client).query(&sql, &[&before, &(limit + 1)]).await;
        rows.map_err(|e| opened.scrub(e))
    };
    let rows = crate::modules::block_on(run).map_err(|e| e.to_string())??;
    let more = rows.len() as i64 > limit;
    let rows = &rows[..rows.len().min(limit as usize)];
    let next = match more {
        true => rows.last().and_then(|row| row.try_get::<_, i64>(0).ok()),
        false => None,
    };
    let records: Vec<Value> = rows.iter().filter_map(record).collect();
    Ok(Page {
        skipped: rows.len() - records.len(),
        records,
        next,
    })
}

/// A row as the API answers it, or `None` when any column fails its check.
#[cfg(feature = "postgres")]
fn record(row: &tokio_postgres::Row) -> Option<Value> {
    let text = |i: usize| row.try_get::<_, Option<String>>(i).ok();
    let one_of = |i: usize, allowed: &[&str]| text(i)?.filter(|v| allowed.contains(&v.as_str()));
    let id = row.try_get::<_, i64>(0).ok().filter(|id| *id > 0)?;
    let time = text(1)?.filter(|t| crate::postgres::is_time(t))?;
    let via = one_of(2, &["console", "cli"])?;
    let (subject, login) = (text(3)?, text(4)?);
    if (login.as_deref()).is_some_and(|l| meada_core::manifest::added_by_problem(l).is_some()) {
        return None;
    }
    let role = one_of(5, &["viewer", "operator", "team_operator"])?;
    let owners = match text(6)? {
        None => Value::Null,
        Some(json) => serde_json::from_str::<Vec<String>>(&json).ok()?.into(),
    };
    let action = one_of(7, &ACTIONS)?;
    let arguments: serde_json::Map<String, Value> = serde_json::from_str(&text(8)??).ok()?;
    let known = ["version", "message", "generation", "at_least"];
    if !(arguments.iter()).all(|(k, v)| known.contains(&k.as_str()) && v.is_string()) {
        return None;
    }
    let outcome = one_of(9, &["pending", "accepted", "refused", "failed"])?;
    Some(json!({
        "id": id, "time": time, "via": via, "subject": subject, "login": login, "role": role,
        "owners": owners, "action": action, "arguments": arguments, "outcome": outcome,
        "message": text(10)?,
    }))
}

/// A CLI twin of a console write under the audit of `manifest`: the record of `action` with
/// `arguments` by `subject`, as the global operator, before `op` runs, then its outcome
/// (`accepted` with `accepted`'s message, `refused` when `refused` says so, else `failed`).
/// `arguments` is `None` when the command's arguments fail their checks: `op` refuses them, as
/// it does a manifest it cannot read, with no record. Postgres unreachable refuses the command
/// with the message naming it, and `op` never runs.
pub fn cli<T, E: std::fmt::Display + Into<Box<dyn std::error::Error>>>(
    manifest: &Path,
    subject: &str,
    action: &str,
    arguments: Option<Value>,
    op: impl FnOnce() -> Result<T, E>,
    accepted: impl FnOnce(&T) -> String,
    refused: impl FnOnce(&E) -> bool,
) -> crate::Result<T> {
    // A manifest that does not read is refused by `op`, which reads it whole (`Manifest::read`,
    // `[postgres]` included), before it writes anything.
    let audit = Audit::of(manifest).unwrap_or_default();
    let (Some(arguments), true) = (arguments, audit.on()) else {
        return op().map_err(Into::into);
    };
    let actor = Actor {
        via: "cli",
        subject: Some(subject),
        login: None,
        role: "operator",
        owners: None,
    };
    let pending = (audit.pending(&actor, action, &arguments))
        .map_err(|reason| Msg::AuditCli(&reason).text())?;
    let done = op();
    match &done {
        Ok(value) => audit.finish(pending, Outcome::Accepted, &accepted(value), None),
        Err(e) => {
            let outcome = Outcome::of(refused(e));
            audit.finish(pending, outcome, &e.to_string(), None);
        }
    }
    done.map_err(Into::into)
}
