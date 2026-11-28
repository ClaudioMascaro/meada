use std::collections::HashSet;

use crate::postgres::Config as Postgres;

// ponytail: the most groups the owner picker lists; a longer prefix narrows it.
pub const PICKER_MAX: i64 = 100;

pub struct Directory {
    postgres: Postgres,
    user_claim: String,
    user_attribute: &'static str,
    group_attribute: &'static str,
}

#[cfg_attr(not(feature = "oidc"), expect(dead_code))]
pub struct Counts {
    pub users: i64,
    pub groups: i64,
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

    #[cfg_attr(not(feature = "oidc"), expect(dead_code))]
    pub fn user_claim(&self) -> &str {
        &self.user_claim
    }

    fn group_column(&self) -> &'static str {
        match self.group_attribute {
            "externalId" => "external_id",
            _ => "display_name",
        }
    }

    pub fn bound(&self) -> std::time::Duration {
        self.postgres.bound(1)
    }

    async fn connect(&self) -> Result<(crate::postgres::Opened, String), String> {
        let opened = crate::postgres::connected(&self.postgres).await?;
        Ok((opened, self.postgres.quoted()))
    }

    pub async fn deprovisioned(&self, claim: &str) -> Result<bool, String> {
        if claim.contains('\0') {
            return Ok(false);
        }
        let (opened, schema) = self.connect().await?;
        let matches = match self.user_attribute {
            "externalId" => "external_id = $1",
            _ => "lower(user_name) = lower($1)",
        };
        let sql = format!(
            "SELECT EXISTS (SELECT 1 FROM {schema}.directory_users WHERE {matches} AND NOT active) \
             OR EXISTS (SELECT 1 FROM {schema}.directory_revoked WHERE {matches})"
        );
        let row = (opened.client).query_one(&sql, &[&claim]).await;
        let row = row.map_err(|e| opened.scrub(e))?;
        row.try_get::<_, bool>(0).map_err(|e| opened.scrub(e))
    }

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
            None => Err(crate::msg::Msg::DirectoryStateMissing.text()),
        }
    }

    #[cfg_attr(not(feature = "oidc"), expect(dead_code))]
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
