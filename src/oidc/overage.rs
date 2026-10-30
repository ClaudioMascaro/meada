//! Entra ID's groups overage through the directory (#336, ADR 0016, `[oidc.entra] overage =
//! "directory"`). A token in overage has no `groups_claim` claim and a `_claim_names` object
//! mapping it to a source (`in_overage`); Meada never calls the source. Its groups are the
//! `group_attribute` values of the groups whose members include the active directory user whose
//! `user_attribute` equals the token's `user_claim` (`userName` ignoring case, `externalId`
//! exactly, as the console matches users): a user not found, inactive or in no group has none.
//!
//! The lookup runs at admission, once per `user_claim` value per `OVERAGE_TTL`: nodes and the
//! console cache the answer (an answer of no groups included) in memory, a failed lookup is not
//! cached, and an expired entry is dropped when found and by a sweep every TTL that a store
//! triggers, so memory follows the subjects seen in the last two TTLs and never grows past
//! them. Only a token that passed every check reaches it. With Postgres unreachable or its schema
//! version unknown, and nothing cached, the token has no groups, as without overage, with one
//! stderr line. Every group value read is hostile: one that fails S3's owner name rule is
//! skipped with one line naming the table.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::msg::Msg;

// ponytail: how long an overage answer is kept, so a membership change reaches a subject at most
// this late; `MEADA_OVERAGE_TTL_MS` shortens it in debug builds.
const OVERAGE_TTL: Duration = Duration::from_secs(300);
/// The statements of one lookup: the two of the version check, and the groups.
const STATEMENTS: u32 = 3;

/// How long an answer is kept. In debug builds `MEADA_OVERAGE_TTL_MS` shortens it (a knob for
/// tests and not documented, as `MEADA_DRAIN_MS` is; a release binary ignores it).
fn ttl() -> Duration {
    #[cfg(debug_assertions)]
    if let Some(ms) = (std::env::var("MEADA_OVERAGE_TTL_MS").ok())
        .and_then(|ms| ms.parse::<u64>().ok())
        .filter(|ms| *ms > 0)
    {
        return Duration::from_millis(ms);
    }
    OVERAGE_TTL
}

/// Whether `claims` is in Entra overage: no claim named `groups_claim`, and a `_claim_names`
/// that is an object mapping `groups_claim` to a string, the source. Anything else is a token
/// without groups, not in overage.
pub(super) fn in_overage(claims: &Map<String, Value>, groups_claim: &str) -> bool {
    !claims.contains_key(groups_claim)
        && (claims.get("_claim_names").and_then(Value::as_object))
            .and_then(|names| names.get(groups_claim))
            .is_some_and(Value::is_string)
}

/// The answers kept, by `user_claim` value, and when they were last swept.
struct Cache {
    answers: HashMap<String, (Instant, Vec<String>)>,
    swept: Instant,
}

impl Cache {
    fn new(now: Instant) -> Self {
        Self {
            answers: HashMap::new(),
            swept: now,
        }
    }

    /// The answer kept for `user` at `now`, dropping it when it is `ttl` old.
    fn get(&mut self, user: &str, now: Instant, ttl: Duration) -> Option<Vec<String>> {
        let (at, groups) = self.answers.get(user)?;
        if now.saturating_duration_since(*at) < ttl {
            return Some(groups.clone());
        }
        self.answers.remove(user);
        None
    }

    /// Keeps `groups` for `user` from `now`; every `ttl`, the entries `ttl` old go, so what is
    /// never asked again is not kept either.
    fn put(&mut self, user: &str, groups: Vec<String>, now: Instant, ttl: Duration) {
        if now.saturating_duration_since(self.swept) >= ttl {
            self.answers
                .retain(|_, (at, _)| now.saturating_duration_since(*at) < ttl);
            self.swept = now;
        }
        self.answers.insert(user.to_string(), (now, groups));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.answers.len()
    }
}

/// The lookup of one process: the directory's settings, the data layer it reads and the cache.
pub(super) struct Overage {
    postgres: crate::postgres::Config,
    /// `[directory] user_claim`: the token claim naming the user.
    user_claim: String,
    /// The SQL condition `user_attribute` names on `directory_users` (`u`), and how it compares.
    user_predicate: &'static str,
    /// The column `group_attribute` names.
    column: &'static str,
    cache: Mutex<Cache>,
}

impl Overage {
    pub(super) fn new(
        directory: &crate::directory::Config,
        postgres: crate::postgres::Config,
    ) -> Self {
        Self {
            postgres,
            user_claim: directory.user_claim.clone(),
            user_predicate: match directory.user_attribute {
                "externalId" => "u.external_id = $1",
                _ => "lower(u.user_name) = lower($1)",
            },
            column: match directory.group_attribute {
                "externalId" => "external_id",
                _ => "display_name",
            },
            cache: Mutex::new(Cache::new(Instant::now())),
        }
    }

    /// The groups of a token in overage whose claims are `claims`: its `user_claim` value's,
    /// from the cache or the directory; none when the claim is no string.
    pub(super) fn groups(&self, claims: &Map<String, Value>) -> Vec<String> {
        let Some(user) = claims.get(&self.user_claim).and_then(Value::as_str) else {
            return Vec::new();
        };
        let ttl = ttl();
        let kept =
            (self.cache.lock().unwrap_or_else(|e| e.into_inner())).get(user, Instant::now(), ttl);
        if let Some(kept) = kept {
            return kept;
        }
        // No stored name holds a NUL, and Postgres refuses one.
        if user.contains('\0') {
            return Vec::new();
        }
        // The answer is as old as the moment the question was asked, not the one it came back:
        // a slow lookup cannot outlive the TTL.
        let asked_at = Instant::now();
        let bound = self.postgres.bound(STATEMENTS);
        let asked = crate::modules::within(bound, self.ask(user), Msg::PostgresTimeout);
        match asked {
            Ok(groups) => {
                let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
                cache.put(user, groups.clone(), asked_at, ttl);
                groups
            }
            Err(why) => {
                eprintln!("{}", Msg::OverageUnreadable(&why).text());
                Vec::new()
            }
        }
    }

    /// The `group_attribute` values of the groups `user` belongs to as an active user, sorted
    /// and without repeats; the schema version refusal or the failure, scrubbed, as the error.
    async fn ask(&self, user: &str) -> Result<Vec<String>, String> {
        let opened = (crate::postgres::versioned(&self.postgres).await).map_err(|(why, _)| why)?;
        let q = self.postgres.quoted();
        let (column, matches) = (self.column, self.user_predicate);
        let sql = format!(
            "SELECT g.{column} FROM {q}.directory_users u \
             JOIN {q}.directory_members m ON m.user_id = u.id \
             JOIN {q}.directory_groups g ON g.id = m.group_id \
             WHERE u.active AND {matches} AND g.{column} IS NOT NULL"
        );
        let rows = (opened.client.query(&sql, &[&user]).await).map_err(|e| opened.scrub(e))?;
        let table = format!("{q}.directory_groups");
        let mut groups: Vec<String> = Vec::new();
        for row in rows {
            match row.try_get::<_, String>(0) {
                Ok(group) if meada_core::manifest::owner_problem(&group).is_none() => {
                    groups.push(group)
                }
                _ => eprintln!("{}", Msg::PermissionsBadRow(&table).text()),
            }
        }
        groups.sort();
        groups.dedup();
        Ok(groups)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn claims(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn overage_is_no_groups_claim_and_a_claim_names_object_mapping_it_to_a_string() {
        let source = json!({"groups": "src1"});
        assert!(in_overage(
            &claims(json!({"_claim_names": source})),
            "groups"
        ));
        for not in [
            json!({}),
            // The claim is present, whatever it holds: never looked up.
            json!({"groups": [], "_claim_names": {"groups": "src1"}}),
            json!({"groups": null, "_claim_names": {"groups": "src1"}}),
            json!({"_claim_names": "groups"}),
            json!({"_claim_names": ["groups"]}),
            json!({"_claim_names": null}),
            json!({"_claim_names": {"groups": 1}}),
            json!({"_claim_names": {"groups": ["src1"]}}),
            json!({"_claim_names": {"groups": null}}),
            json!({"_claim_names": {"roles": "src1"}}),
            json!({"_claim_names": {}}),
        ] {
            assert!(!in_overage(&claims(not.clone()), "groups"), "{not}");
        }
        // The claim the section names, not `groups`.
        let other = claims(json!({"_claim_names": {"groups": "src1"}}));
        assert!(!in_overage(&other, "teams"));
    }

    #[test]
    fn an_answer_lives_one_ttl_and_a_sweep_drops_what_expired() {
        let ttl = Duration::from_secs(10);
        let start = Instant::now();
        let mut cache = Cache::new(start);
        cache.put("a", vec!["g".into()], start, ttl);
        // An answer of no groups is kept like any other.
        cache.put("b", Vec::new(), start, ttl);
        let almost = start + Duration::from_secs(9);
        assert_eq!(cache.get("a", almost, ttl), Some(vec!["g".to_string()]));
        assert_eq!(cache.get("b", almost, ttl), Some(Vec::new()));
        assert_eq!(cache.get("c", almost, ttl), None);
        let expired = start + ttl;
        assert_eq!(cache.get("a", expired, ttl), None);
        assert_eq!(cache.len(), 1, "an expired entry is dropped when found");
        // `b` is never asked again: the next store after a TTL sweeps it.
        cache.put("c", Vec::new(), expired, ttl);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("c", expired, ttl), Some(Vec::new()));
        // A store within a TTL of the last sweep sweeps nothing.
        cache.put("d", Vec::new(), expired + Duration::from_secs(1), ttl);
        assert_eq!(cache.len(), 2);
    }
}
