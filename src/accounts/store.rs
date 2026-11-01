//! The accounts table (ADR 0024, migration 9): one row per local account in `[postgres] schema`,
//! read and written by the console, and by `meada ui reset-admin` (`reset_admin`). Each call
//! opens its own connection through `postgres::connected`, so a schema version this binary does
//! not know counts as unreachable, and is bounded as the audit's calls are. Every row is hostile when read: one that fails its
//! checks admits nothing (`Attempt::Bad`), and its reader writes one line naming the table, never
//! a value. A password attempt (`attempt`) holds the row while it checks and counts. Account
//! administration (`list`, `create`, `change`, #557, groups #558) is the global operator's; a
//! change that could leave no active operator runs under the schema's advisory lock, so two at
//! once never do.
//! A console write first reads its account's `standing`.

use crate::accounts::{Role, groups_ok, is_hash, username_of, username_ok};
use crate::modules::Problem;
use crate::msg::Msg;
use crate::postgres::Config;

/// An account as its row holds it, checked.
pub struct Account {
    pub username: String,
    /// Its argon2id PHC string.
    pub hash: String,
    pub role: Role,
    pub groups: Vec<String>,
    pub change_required: bool,
    pub suspended: bool,
}

/// Why an attempt on an account's password is refused, as the audit records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No account of that username, or a row that fails its checks.
    Unknown,
    /// The password is not the account's.
    Password,
    /// Suspended: the password was not checked.
    Suspended,
    /// Locked by earlier failures: the password was not checked.
    Locked,
    /// The password is not the account's, and this failure locked it.
    LockedNow,
    /// Under break glass, a username other than the default account's: the table is not read and
    /// the password not checked (#563).
    NotBreakGlass,
}

impl Refusal {
    /// The `reason` of the audit record and the stderr line.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::Unknown => "unknown",
            Refusal::Password => "password",
            Refusal::Suspended => "suspended",
            Refusal::Locked => "locked",
            Refusal::LockedNow => "locked_now",
            Refusal::NotBreakGlass => "not_break_glass",
        }
    }
}

/// What an attempt on the password of a username found and did.
pub enum Attempt {
    /// No row of that username.
    Unknown,
    /// A row that fails its checks: it admits nothing.
    Bad,
    /// The check could not hash (no permit left): nothing counted.
    Busy,
    /// Refused before its password was checked (`Locked`, `Suspended`): the row released at once,
    /// nothing counted, and the caller hashes the dummy so the refusal costs what any other does.
    Unchecked(Account, Refusal),
    /// The password checked under the row's lock: right (`None`, the count cleared), or wrong
    /// (`Password`, or `LockedNow` for the failure that reached `LOCKOUT_FAILURES`).
    Checked(Account, Option<Refusal>),
}

/// The table, as a stderr line names it.
pub fn table(config: &Config) -> String {
    format!("{}.accounts", config.quoted())
}

/// An attempt on the password of `username`, in one transaction that holds its row
/// (`SELECT ... FOR UPDATE`), so attempts in parallel are counted one after another and never
/// pass `LOCKOUT_FAILURES`: a locked or suspended account is `Unchecked` (a lock is never
/// extended); otherwise `check` hashes the password against the stored hash (`None` when it
/// cannot: `Busy`, nothing counted). A wrong one adds a failure, and the one that reaches
/// `LOCKOUT_FAILURES` locks the account for `LOCKOUT_MINUTES` and starts the count again; a right
/// one clears the count.
pub fn attempt(
    config: &Config,
    username: &str,
    check: impl FnOnce(&str) -> Option<bool>,
) -> Result<Attempt, String> {
    let quoted = config.quoted();
    let run = async {
        let mut opened = crate::postgres::connected(config).await?;
        let scrub = opened.scrubber();
        let tx = opened.client.transaction().await.map_err(&scrub)?;
        let sql = format!(
            "SELECT username, password_hash, role, groups, change_required, suspended, \
             locked_until IS NOT NULL AND locked_until > now(), failures FROM {quoted}.accounts \
             WHERE username = $1 FOR UPDATE"
        );
        let rows = tx.query(&sql, &[&username]).await.map_err(&scrub)?;
        let Some(row) = rows.first() else {
            return Ok(Attempt::Unknown);
        };
        let Some(account) = checked(row, username) else {
            return Ok(Attempt::Bad);
        };
        let locked: bool = row.try_get(6).map_err(&scrub)?;
        let failures: i32 = row.try_get(7).map_err(&scrub)?;
        if locked {
            return Ok(Attempt::Unchecked(account, Refusal::Locked));
        }
        if account.suspended {
            return Ok(Attempt::Unchecked(account, Refusal::Suspended));
        }
        let Some(right) = check(&account.hash) else {
            return Ok(Attempt::Busy);
        };
        let refusal = match (right, failures) {
            (true, 0) => None,
            (true, _) => {
                let sql = format!(
                    "UPDATE {quoted}.accounts SET failures = 0, locked_until = NULL \
                     WHERE username = $1"
                );
                tx.execute(&sql, &[&username]).await.map_err(&scrub)?;
                None
            }
            (false, _) => {
                let sql = format!(
                    "UPDATE {quoted}.accounts SET \
                     locked_until = CASE WHEN failures + 1 >= $2 \
                       THEN now() + make_interval(mins => $3) ELSE locked_until END, \
                     failures = CASE WHEN failures + 1 >= $2 THEN 0 ELSE failures + 1 END \
                     WHERE username = $1 RETURNING failures = 0"
                );
                let limits = (
                    crate::accounts::LOCKOUT_FAILURES,
                    crate::accounts::LOCKOUT_MINUTES,
                );
                let values: [&(dyn tokio_postgres::types::ToSql + Sync); 3] =
                    [&username, &limits.0, &limits.1];
                let row = tx.query_one(&sql, &values).await.map_err(&scrub)?;
                let locked_now: bool = row.try_get(0).map_err(&scrub)?;
                Some(if locked_now {
                    Refusal::LockedNow
                } else {
                    Refusal::Password
                })
            }
        };
        tx.commit().await.map_err(&scrub)?;
        Ok(Attempt::Checked(account, refusal))
    };
    // Waiting for the row is one statement's bound, the hash under it another.
    crate::modules::within(config.bound(5), run, Msg::PostgresTimeout)
}

/// `row`, the one `username` named, when every column passes its check.
fn checked(row: &tokio_postgres::Row, username: &str) -> Option<Account> {
    let name: String = row.try_get(0).ok()?;
    let hash: String = row.try_get(1).ok()?;
    let role = Role::of(&row.try_get::<_, String>(2).ok()?)?;
    let groups: Vec<String> = row.try_get(3).ok()?;
    if name != username || !username_ok(&name) || !is_hash(&hash) || !groups_ok(&groups) {
        return None;
    }
    Some(Account {
        username: name,
        hash,
        role,
        groups,
        change_required: row.try_get(4).ok()?,
        suspended: row.try_get(5).ok()?,
    })
}

/// Replaces the stored hash `old` of `username` by `new`, the same password under the console's
/// own parameters (a sign-in that found `old` weaker): only while `old` is still the one stored,
/// so a password changed meanwhile is never undone. Whether it was replaced.
pub fn rehash(config: &Config, username: &str, old: &str, new: &str) -> Result<bool, String> {
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        let sql = format!(
            "UPDATE {}.accounts SET password_hash = $3 WHERE username = $1 AND password_hash = $2",
            config.quoted()
        );
        let done = (opened.client)
            .execute(&sql, &[&username, &old, &new])
            .await;
        Ok(done.map_err(|e| opened.scrub(e))? == 1)
    };
    crate::modules::within(config.bound(1), run, Msg::PostgresTimeout)
}

/// Stores `hash` as the password of `username`, clears its change required and records the time:
/// whether an active account took it.
pub fn set_password(config: &Config, username: &str, hash: &str) -> Result<bool, String> {
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        let sql = format!(
            "UPDATE {}.accounts SET password_hash = $2, change_required = false, \
             password_changed = now() WHERE username = $1 AND NOT suspended",
            config.quoted()
        );
        let done = (opened.client).execute(&sql, &[&username, &hash]).await;
        Ok(done.map_err(|e| opened.scrub(e))? == 1)
    };
    crate::modules::within(config.bound(3), run, Msg::PostgresTimeout)
}

/// `meada ui reset-admin`: `admin` created, or made an active operator again with its lock and
/// count cleared, its password `hash` with a change required. Whether it was created.
pub fn reset_admin(config: &Config, hash: &str) -> Result<bool, String> {
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        let sql = format!(
            "INSERT INTO {}.accounts AS a (username, password_hash, role, change_required) \
             VALUES ($1, $2, 'operator', true) ON CONFLICT (username) DO UPDATE SET \
             password_hash = EXCLUDED.password_hash, role = 'operator', change_required = true, \
             suspended = false, failures = 0, locked_until = NULL, password_changed = now() \
             RETURNING a.xmax = 0",
            // `xmax` is 0 on a row this statement inserted, the transaction's id on one it
            // updated: Postgres's own way to tell the two arms of an upsert apart.
            config.quoted()
        );
        let username = crate::accounts::DEFAULT_USERNAME;
        let row = (opened.client).query_one(&sql, &[&username, &hash]).await;
        let row = row.map_err(|e| opened.scrub(e))?;
        row.try_get::<_, bool>(0).map_err(|e| opened.scrub(e))
    };
    crate::modules::within(config.bound(1), run, Msg::PostgresTimeout)
}

/// Why the default account could not be made at start.
pub enum Refused {
    /// Postgres unreachable, or a schema version this binary does not know.
    Postgres(String),
    /// The initial password the deployer set: its key and the problem.
    Password(Problem),
}

/// The advisory lock of `schema`'s default account: two consoles started together create one.
fn lock_key(schema: &str) -> i64 {
    let hash = blake3::hash(format!("meada accounts {schema}").as_bytes());
    i64::from_le_bytes(hash.as_bytes()[..8].try_into().expect("eight bytes"))
}

/// The console's start: when the table is empty, the default account `admin`, an operator, with
/// the hash and change required `initial` gives, in one transaction under an advisory lock, so
/// two consoles started together create one row. A table that is not empty is never touched, and
/// `initial` (which reads the deployer's password) is then never called. Whether it created it.
pub fn bootstrap(
    config: &Config,
    initial: impl FnOnce() -> Result<(String, bool), Problem>,
) -> Result<bool, Refused> {
    let quoted = config.quoted();
    let key = lock_key(config.schema());
    let run = async {
        let mut opened = crate::postgres::connected(config).await?;
        let scrub = opened.scrubber();
        let tx = opened.client.transaction().await.map_err(&scrub)?;
        let lock = tx
            .execute("SELECT pg_advisory_xact_lock($1)", &[&key])
            .await;
        lock.map_err(&scrub)?;
        let any = format!("SELECT EXISTS (SELECT 1 FROM {quoted}.accounts)");
        let any: bool = tx.query_one(&any, &[]).await.map_err(&scrub)?.get(0);
        if any {
            return Ok(Ok(false));
        }
        let (hash, change_required) = match initial() {
            Ok(initial) => initial,
            Err(problem) => return Ok(Err(problem)),
        };
        let insert = format!(
            "INSERT INTO {quoted}.accounts (username, password_hash, role, change_required) \
             VALUES ($1, $2, 'operator', $3)"
        );
        let username = crate::accounts::DEFAULT_USERNAME;
        let values: [&(dyn tokio_postgres::types::ToSql + Sync); 3] =
            [&username, &hash, &change_required];
        tx.execute(&insert, &values).await.map_err(&scrub)?;
        tx.commit().await.map_err(&scrub)?;
        Ok(Ok(true))
    };
    match crate::modules::within(config.bound(6), run, Msg::PostgresTimeout) {
        Ok(Ok(created)) => Ok(created),
        Ok(Err(problem)) => Err(Refused::Password(problem)),
        Err(reason) => Err(Refused::Postgres(reason)),
    }
}

/// How a timestamp column reads: RFC 3339 in UTC to the microsecond, as the audit's times.
const TIME: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'";

/// The columns of an account as the list shows it, in `Listed`'s order, then its hash, read only
/// to check the row as a sign-in would (`Listed` never holds it), and its groups. A lock that has
/// ended reads as none.
fn listed_columns() -> String {
    format!(
        "username, role, suspended, CASE WHEN locked_until > now() THEN \
         to_char(locked_until AT TIME ZONE 'UTC', {TIME}) END, change_required, \
         to_char(password_changed AT TIME ZONE 'UTC', {TIME}), \
         to_char(created AT TIME ZONE 'UTC', {TIME}), created_by, password_hash, groups"
    )
}

/// An account as the global operator's list shows it (#557): never its hash.
#[derive(Debug, PartialEq)]
pub struct Listed {
    pub username: String,
    pub role: Role,
    pub suspended: bool,
    /// When its lock ends, while one holds.
    pub locked_until: Option<String>,
    pub change_required: bool,
    pub password_changed: String,
    pub created: String,
    /// The subject of the global operator that created it; none for the default account and
    /// one `meada ui reset-admin` created.
    pub created_by: Option<String>,
    /// Its groups, as stored (#558).
    pub groups: Vec<String>,
}

impl Listed {
    /// Its state: `suspended`, else `locked` while a lock holds, else `active`.
    pub fn state(&self) -> &'static str {
        match (self.suspended, &self.locked_until) {
            (true, _) => "suspended",
            (false, Some(_)) => "locked",
            (false, None) => "active",
        }
    }
}

/// `row`, read with `listed_columns`, when every column passes its check.
fn listed(row: &tokio_postgres::Row) -> Option<Listed> {
    let time = |i: usize| -> Option<Option<String>> {
        let text: Option<String> = row.try_get(i).ok()?;
        match text {
            Some(t) if !crate::postgres::is_time(&t) => None,
            text => Some(text),
        }
    };
    let username: String = row.try_get(0).ok()?;
    let role = Role::of(&row.try_get::<_, String>(1).ok()?)?;
    let created_by: Option<String> = row.try_get(7).ok()?;
    let hash: String = row.try_get(8).ok()?;
    let groups: Vec<String> = row.try_get(9).ok()?;
    let creator_ok = created_by
        .as_deref()
        .is_none_or(|s| username_of(s).is_some());
    if !username_ok(&username) || !creator_ok || !is_hash(&hash) || !groups_ok(&groups) {
        return None;
    }
    Some(Listed {
        username,
        role,
        suspended: row.try_get(2).ok()?,
        locked_until: time(3)?,
        change_required: row.try_get(4).ok()?,
        password_changed: time(5)??,
        created: time(6)??,
        created_by,
        groups,
    })
}

/// Every account, by username, and how many rows failed their checks and were left out.
pub fn list(config: &Config) -> Result<(Vec<Listed>, usize), String> {
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        let sql = format!(
            "SELECT {} FROM {}.accounts ORDER BY username",
            listed_columns(),
            config.quoted()
        );
        let rows = (opened.client).query(&sql, &[]).await;
        rows.map_err(|e| opened.scrub(e))
    };
    let rows = crate::modules::within(config.bound(2), run, Msg::PostgresTimeout)?;
    let accounts: Vec<Listed> = rows.iter().filter_map(listed).collect();
    let skipped = rows.len() - accounts.len();
    Ok((accounts, skipped))
}

/// A new account `username` of `role` in `groups`, its password `hash`, a change required when
/// `change_required`, created by `created_by` (a subject): `Taken` when the username is.
pub fn create(
    config: &Config,
    username: &str,
    role: Role,
    groups: &[String],
    hash: &str,
    change_required: bool,
    created_by: &str,
) -> Result<Changed, String> {
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        let sql = format!(
            "INSERT INTO {}.accounts (username, password_hash, role, change_required, \
             created_by, groups) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (username) DO \
             NOTHING RETURNING {}",
            config.quoted(),
            listed_columns()
        );
        let values: [&(dyn tokio_postgres::types::ToSql + Sync); 6] = [
            &username,
            &hash,
            &role.name(),
            &change_required,
            &created_by,
            &groups,
        ];
        let rows = (opened.client).query(&sql, &values).await;
        let rows = rows.map_err(|e| opened.scrub(e))?;
        Ok(match rows.first() {
            Some(row) => Changed::Done(listed(row)),
            None => Changed::Taken,
        })
    };
    crate::modules::within(config.bound(2), run, Msg::PostgresTimeout)
}

/// One change of account administration (#557).
pub enum Change {
    /// The role set.
    Role(Role),
    /// The groups set, each an owner name (#558).
    Groups(Vec<String>),
    Suspend,
    Reactivate,
    /// The password `hash` set, a change required when `change_required`, the lock and its
    /// count cleared.
    ResetPassword {
        hash: String,
        change_required: bool,
    },
}

/// What a creation or a change found and did.
#[derive(Debug, PartialEq)]
pub enum Changed {
    /// A creation's username is another account's: nothing created.
    Taken,
    /// No row of that username.
    Unknown,
    /// A row that fails its checks: left as it was.
    Bad,
    /// The change would leave no active operator: nothing changed.
    LastOperator,
    /// Done: the account as listed after it, `None` when that row fails its checks.
    Done(Option<Listed>),
}

/// `change` of the account `username`, in one transaction under the schema's advisory lock, so
/// two changes at once are decided one after the other: a suspension or a demotion of the last
/// active operator is `LastOperator`, and nothing changes.
pub fn change(config: &Config, username: &str, change: &Change) -> Result<Changed, String> {
    let quoted = config.quoted();
    let key = lock_key(config.schema());
    let run = async {
        let mut opened = crate::postgres::connected(config).await?;
        let scrub = opened.scrubber();
        let tx = opened.client.transaction().await.map_err(&scrub)?;
        let lock = tx
            .execute("SELECT pg_advisory_xact_lock($1)", &[&key])
            .await;
        lock.map_err(&scrub)?;
        let sql =
            format!("SELECT role, suspended FROM {quoted}.accounts WHERE username = $1 FOR UPDATE");
        let rows = tx.query(&sql, &[&username]).await.map_err(&scrub)?;
        let Some(row) = rows.first() else {
            return Ok(Changed::Unknown);
        };
        let role = row.try_get::<_, String>(0).ok().and_then(|r| Role::of(&r));
        let suspended = row.try_get::<_, bool>(1).ok();
        let (Some(role), Some(suspended)) = (role, suspended) else {
            return Ok(Changed::Bad);
        };
        let removes = matches!(change, Change::Role(Role::Viewer) | Change::Suspend);
        if removes && role == Role::Operator && !suspended {
            // The other active operators that can sign in: a row failing its checks admits
            // nothing, so it never counts as one.
            let others = format!(
                "SELECT username, password_hash, role, groups, change_required, suspended \
                 FROM {quoted}.accounts WHERE role = $2 AND NOT suspended AND username <> $1"
            );
            let operator = Role::Operator.name();
            let others = tx.query(&others, &[&username, &operator]).await;
            let others = others.map_err(&scrub)?;
            if !(others.iter()).any(|row| {
                row.try_get(0)
                    .is_ok_and(|name: String| checked(row, &name).is_some())
            }) {
                return Ok(Changed::LastOperator);
            }
        }
        let columns = listed_columns();
        let set = |set: &str| {
            format!("UPDATE {quoted}.accounts SET {set} WHERE username = $1 RETURNING {columns}")
        };
        let row = match change {
            Change::Role(role) => {
                let sql = set("role = $2");
                tx.query_one(&sql, &[&username, &role.name()]).await
            }
            Change::Groups(groups) => {
                let sql = set("groups = $2");
                tx.query_one(&sql, &[&username, groups]).await
            }
            Change::Suspend => tx.query_one(&set("suspended = true"), &[&username]).await,
            Change::Reactivate => tx.query_one(&set("suspended = false"), &[&username]).await,
            Change::ResetPassword {
                hash,
                change_required,
            } => {
                let sql = set("password_hash = $2, change_required = $3, failures = 0, \
                     locked_until = NULL, password_changed = now()");
                let values: [&(dyn tokio_postgres::types::ToSql + Sync); 3] =
                    [&username, hash, change_required];
                tx.query_one(&sql, &values).await
            }
        };
        let done = listed(&row.map_err(&scrub)?);
        tx.commit().await.map_err(&scrub)?;
        Ok(Changed::Done(done))
    };
    // Waiting for the lock is one statement's bound, the change under it a few more.
    crate::modules::within(config.bound(5), run, Msg::PostgresTimeout)
}

/// An account's standing, read before each of its console writes (ADR 0024).
#[derive(Debug, PartialEq)]
pub enum Standing {
    /// No row of that username.
    Unknown,
    /// A row that fails its checks: it admits nothing.
    Bad,
    Suspended,
    /// Active, with its role and groups now.
    Active(Role, Vec<String>),
}

/// The standing of the account `username` now.
pub fn standing(config: &Config, username: &str) -> Result<Standing, String> {
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        let sql = format!(
            "SELECT role, groups, suspended FROM {}.accounts WHERE username = $1",
            config.quoted()
        );
        let rows = (opened.client).query(&sql, &[&username]).await;
        rows.map_err(|e| opened.scrub(e))
    };
    let rows = crate::modules::within(config.bound(1), run, Msg::PostgresTimeout)?;
    let Some(row) = rows.first() else {
        return Ok(Standing::Unknown);
    };
    let role = row.try_get::<_, String>(0).ok().and_then(|r| Role::of(&r));
    let groups = row.try_get::<_, Vec<String>>(1).ok();
    let suspended = row.try_get::<_, bool>(2).ok();
    Ok(match (role, groups, suspended) {
        (Some(role), Some(groups), Some(false)) if groups_ok(&groups) => {
            Standing::Active(role, groups)
        }
        (Some(_), Some(groups), Some(true)) if groups_ok(&groups) => Standing::Suspended,
        _ => Standing::Bad,
    })
}
