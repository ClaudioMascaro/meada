use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::msg::Msg;

// ponytail: how long an overage answer is kept, so a membership change reaches a subject at most
// this late; `MEADA_OVERAGE_TTL_MS` shortens it in debug builds.
const OVERAGE_TTL: Duration = Duration::from_secs(300);

const STATEMENTS: u32 = 1;

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

pub(super) fn in_overage(claims: &Map<String, Value>, groups_claim: &str) -> bool {
    !claims.contains_key(groups_claim)
        && (claims.get("_claim_names").and_then(Value::as_object))
            .and_then(|names| names.get(groups_claim))
            .is_some_and(Value::is_string)
}

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

    fn get(&mut self, user: &str, now: Instant, ttl: Duration) -> Option<Vec<String>> {
        let (at, groups) = self.answers.get(user)?;
        if now.saturating_duration_since(*at) < ttl {
            return Some(groups.clone());
        }
        self.answers.remove(user);
        None
    }

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

pub(super) struct Overage {
    postgres: crate::postgres::Config,
    user_claim: String,
    user_predicate: &'static str,
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
        if user.contains('\0') {
            return Vec::new();
        }
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
        let other = claims(json!({"_claim_names": {"groups": "src1"}}));
        assert!(!in_overage(&other, "teams"));
    }

    #[test]
    fn an_answer_lives_one_ttl_and_a_sweep_drops_what_expired() {
        let ttl = Duration::from_secs(10);
        let start = Instant::now();
        let mut cache = Cache::new(start);
        cache.put("a", vec!["g".into()], start, ttl);
        cache.put("b", Vec::new(), start, ttl);
        let almost = start + Duration::from_secs(9);
        assert_eq!(cache.get("a", almost, ttl), Some(vec!["g".to_string()]));
        assert_eq!(cache.get("b", almost, ttl), Some(Vec::new()));
        assert_eq!(cache.get("c", almost, ttl), None);
        let expired = start + ttl;
        assert_eq!(cache.get("a", expired, ttl), None);
        assert_eq!(cache.len(), 1, "an expired entry is dropped when found");
        cache.put("c", Vec::new(), expired, ttl);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("c", expired, ttl), Some(Vec::new()));
        cache.put("d", Vec::new(), expired + Duration::from_secs(1), ttl);
        assert_eq!(cache.len(), 2);
    }
}
