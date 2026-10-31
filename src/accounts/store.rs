//! The accounts table (ADR 0024, migration 9): one row per local account in `[postgres] schema`,
//! read and written by the console alone. Each call opens its own connection through
//! `postgres::connected`, so a schema version this binary does not know counts as unreachable,
//! and is bounded as the audit's calls are. Every row is hostile when read: one that fails its
//! checks admits nothing (`Found::Bad`), and its reader writes one line naming the table, never a
//! value.

use crate::accounts::{Role, groups_ok, is_hash, username_ok};
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

/// What a lookup by username finds.
pub enum Found {
    Account(Account),
    /// No row of that username.
    Unknown,
    /// A row that fails its checks.
    Bad,
}

/// The table, as a stderr line names it.
pub fn table(config: &Config) -> String {
    format!("{}.accounts", config.quoted())
}

/// The account `username` names, read now.
pub fn find(config: &Config, username: &str) -> Result<Found, String> {
    let run = async {
        let opened = crate::postgres::connected(config).await?;
        let sql = format!(
            "SELECT username, password_hash, role, groups, change_required, suspended FROM \
             {}.accounts WHERE username = $1",
            config.quoted()
        );
        let rows = (opened.client).query(&sql, &[&username]).await;
        let rows = rows.map_err(|e| opened.scrub(e))?;
        Ok(match rows.first() {
            None => Found::Unknown,
            Some(row) => checked(row, username).map_or(Found::Bad, Found::Account),
        })
    };
    crate::modules::within(config.bound(3), run, Msg::PostgresTimeout)
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
