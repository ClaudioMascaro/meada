//! The directory's readers (#335, feature `ui`): what the console asks of the users and groups
//! the identity provider pushed in (`scim`). Each call opens its own connection through
//! `postgres::connected`, so Postgres unreachable or a schema version this binary does not know
//! is an `Err` with the scrubbed reason, never the URL. No call caches: the console already
//! reaches Postgres per write for the audit.

use std::collections::HashSet;

use crate::postgres::Config as Postgres;

/// The most groups the owner picker lists.
pub const PICKER_MAX: i64 = 100;

/// The directory as the console reads it.
pub struct Directory {
    postgres: Postgres,
    /// The token claim naming a user (`[directory] user_claim`).
    user_claim: String,
    user_attribute: &'static str,
    group_attribute: &'static str,
}

/// What the admission view shows of the directory.
#[cfg_attr(not(feature = "oidc"), allow(dead_code))] // only the admission view, an oidc route, shows them
pub struct Counts {
    pub users: i64,
    pub groups: i64,
    /// The time of the last SCIM write, RFC 3339 UTC to the microsecond; none before the first.
    pub last_write: Option<String>,
}

impl Directory {
    pub fn new(config: &super::Config, postgres: Postgres) -> Self {
        Self {
            postgres,
            user_claim: config.user_claim.clone(),
            user_attribute: config.user_attribute,
            group_attribute: config.group_attribute,
        }
    }

    /// The token claim that names a user.
    #[cfg_attr(not(feature = "oidc"), allow(dead_code))]
    pub fn user_claim(&self) -> &str {
        &self.user_claim
    }

    /// The column `group_attribute` names.
    fn group_column(&self) -> &'static str {
        match self.group_attribute {
            "externalId" => "external_id",
            _ => "display_name",
        }
    }

    async fn connect(&self) -> Result<(crate::postgres::Opened, String), String> {
        let opened = crate::postgres::connected(&self.postgres).await?;
        Ok((opened, self.postgres.quoted()))
    }

    /// Whether the user `claim` names is deprovisioned: found inactive by `user_attribute` (a
    /// `userName` ignoring case, as SCIM's uniqueness does; an `externalId` exactly), or deleted
    /// after it was provisioned, which its revocation marker records until an active write
    /// clears it. A user the directory never held is not.
    pub async fn deprovisioned(&self, claim: &str) -> Result<bool, String> {
        // No stored name holds a NUL, and Postgres refuses one.
        if claim.contains('\0') {
            return Ok(false);
        }
        let (opened, schema) = self.connect().await?;
        let (user, marker) = match self.user_attribute {
            "externalId" => ("external_id = $1", "external_id = $1"),
            _ => (
                "lower(user_name) = lower($1)",
                "lower(user_name) = lower($1)",
            ),
        };
        let sql = format!(
            "SELECT EXISTS (SELECT 1 FROM {schema}.directory_users WHERE {user} AND NOT active) \
             OR EXISTS (SELECT 1 FROM {schema}.directory_revoked WHERE {marker})"
        );
        let row = (opened.client).query_one(&sql, &[&claim]).await;
        let row = row.map_err(|e| opened.scrub(e))?;
        row.try_get::<_, bool>(0).map_err(|e| opened.scrub(e))
    }

    /// At most `PICKER_MAX` `group_attribute` values starting with `prefix`, compared exactly,
    /// sorted by byte.
    pub async fn groups(&self, prefix: &str) -> Result<Vec<String>, String> {
        let (opened, schema) = self.connect().await?;
        let column = self.group_column();
        let sql = format!(
            "SELECT DISTINCT {column} COLLATE \"C\" FROM {schema}.directory_groups WHERE \
             {column} IS NOT NULL AND starts_with({column}, $1) ORDER BY 1 LIMIT $2"
        );
        let rows = (opened.client).query(&sql, &[&prefix, &PICKER_MAX]).await;
        let rows = rows.map_err(|e| opened.scrub(e))?;
        Ok(rows.iter().filter_map(|r| r.try_get(0).ok()).collect())
    }

    /// Which of `owners` name a group, or `None` while no group was ever provisioned: a
    /// directory whose final group was deleted still answers `Some`, with none of them.
    pub async fn named(&self, owners: &[String]) -> Result<Option<HashSet<String>>, String> {
        let (opened, schema) = self.connect().await?;
        let column = self.group_column();
        let sql = format!(
            "SELECT (SELECT groups_provisioned FROM {schema}.directory_state), \
             ARRAY(SELECT {column} FROM {schema}.directory_groups WHERE {column} = ANY($1))"
        );
        let row = (opened.client).query_one(&sql, &[&owners]).await;
        let row = row.map_err(|e| opened.scrub(e))?;
        let provisioned = row
            .try_get::<_, Option<bool>>(0)
            .map_err(|e| opened.scrub(e))?;
        let named = row.try_get::<_, Vec<Option<String>>>(1);
        let named = named.map_err(|e| opened.scrub(e))?;
        match provisioned {
            Some(true) => Ok(Some(named.into_iter().flatten().collect())),
            Some(false) => Ok(None),
            None => Err("the directory's state is missing".into()),
        }
    }

    /// The users and groups held and the time of the last SCIM write.
    #[cfg_attr(not(feature = "oidc"), allow(dead_code))]
    pub async fn counts(&self) -> Result<Counts, String> {
        let (opened, schema) = self.connect().await?;
        let sql = format!(
            "SELECT (SELECT count(*) FROM {schema}.directory_users), \
             (SELECT count(*) FROM {schema}.directory_groups), \
             (SELECT to_char(last_write AT TIME ZONE 'UTC', \
             'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') FROM {schema}.directory_state)"
        );
        let row = (opened.client).query_one(&sql, &[]).await;
        let row = row.map_err(|e| opened.scrub(e))?;
        let get = |e: tokio_postgres::Error| opened.scrub(e);
        Ok(Counts {
            users: row.try_get(0).map_err(get)?,
            groups: row.try_get(1).map_err(get)?,
            last_write: row.try_get(2).map_err(get)?,
        })
    }
}
