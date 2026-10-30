//! The GitHub grant at admission (#332, ADR 0016): a process with `[oidc.github]` keeps the
//! newest permissions set `meada github-app roles` wrote in memory, keyed by login. A thread
//! reads, every `poll_every()` (`PERMISSIONS_POLL`, 60 seconds), the newest set's id, its
//! `finished` and its age on Postgres's clock, and loads its rows whose `role` is in `roles` only
//! when the id changed, in one read-only transaction that sees one set. Admission never waits on
//! Postgres: before the first read the GitHub grant is empty. The console also reads the set's
//! exclusions with the version it checked, for the workspace views' `excluded` flag (#333).
//!
//! The age is Postgres's current time minus `finished` at the last successful read, plus the
//! process's monotonic time since; a later read of the same set never lowers it, so a clock moved
//! back cannot extend a set's authority. Past `max_age_seconds` the GitHub grant is empty. A
//! failed read is one stderr line per poll and keeps the set aging; a schema version this binary
//! does not know is written once and counts as unreachable. Every row is hostile: a set or a row
//! that fails its checks is skipped with one line naming its table, never a value.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, RwLock, Weak};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio_postgres::IsolationLevel;

use super::{Github, role_ok};
use crate::msg::Msg;

// ponytail: how often the newest set is read, so a revocation on GitHub reaches a process at most
// this long after the refresh that recorded it; `MEADA_PERMISSIONS_POLL_MS` shortens it in debug
// builds.
const PERMISSIONS_POLL: Duration = Duration::from_secs(60);
/// The statements of one read: the version, the newest set, its counts, its rows and its
/// exclusions, and with `[catalog]` its catalog.
const STATEMENTS: u32 = 5 + cfg!(feature = "workspaces") as u32;

// ponytail: the longest identity the catalog holds, in bytes, so that its pagination cursor (the
// identity in base64, at most 256 characters) names any of them; a longer row counts as hostile.
#[cfg(feature = "workspaces")]
const CATALOG_IDENTITY_MAX: usize = 192;

/// How often a process reads the newest permissions set. In debug builds
/// `MEADA_PERMISSIONS_POLL_MS` shortens it (a knob for tests and not documented, as
/// `MEADA_DRAIN_MS` is; a release binary ignores it), and `super::max_age_min` lowers the floor
/// of `max_age_seconds` with it.
pub(super) fn poll_every() -> Duration {
    #[cfg(debug_assertions)]
    if let Some(ms) = poll_knob() {
        return Duration::from_millis(ms);
    }
    PERMISSIONS_POLL
}

/// `MEADA_PERMISSIONS_POLL_MS`, when set to a positive number of milliseconds.
#[cfg(debug_assertions)]
pub(super) fn poll_knob() -> Option<u64> {
    let ms = std::env::var("MEADA_PERMISSIONS_POLL_MS").ok()?;
    ms.parse().ok().filter(|ms| *ms > 0)
}

/// The cache of one process: `[oidc.github]`, the data layer it reads and what it last read.
pub(super) struct Permissions {
    github: Github,
    postgres: crate::postgres::Config,
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    set: Option<Set>,
    /// Whether the schema version refusal was written: it is written once.
    version_said: bool,
}

/// A permissions set as loaded; what only the console shows is unread on nodes.
#[cfg_attr(not(feature = "ui"), allow(dead_code))]
struct Set {
    id: String,
    /// When the job finished it, RFC 3339 on Postgres's clock.
    finished: String,
    /// Its age at `read`.
    age: Duration,
    read: Instant,
    /// The counts as the job wrote them: repositories listed, every row, the exclusions.
    repositories: i64,
    rows: i64,
    exclusions: i64,
    /// The identities each login reads through an accepted role.
    logins: HashMap<String, Vec<String>>,
    /// The workspace version the set was checked against, and the identities of its additions
    /// the set left out (`excluded`).
    version: Option<String>,
    excluded: BTreeSet<String>,
    /// The identities the set left out of each named workspace, by workspace id (#342): the
    /// sponsor rule's exclusions.
    #[cfg(feature = "workspaces")]
    context_excluded: HashMap<String, BTreeSet<String>>,
    /// With `[catalog]` (#341), the identities the catalog holds: the set's `catalog` and `both`
    /// repositories GitHub showed (`listed`). Coverage, not entitlement.
    #[cfg(feature = "workspaces")]
    catalog: BTreeSet<String>,
}

impl Set {
    fn age(&self, now: Instant) -> Duration {
        self.age + now.saturating_duration_since(self.read)
    }

    /// Another read of this set at `now` found `age` on Postgres's clock: the age never moves
    /// back, whatever either clock did.
    fn seen(&mut self, age: Duration, now: Instant) {
        self.age = age.max(self.age(now));
        self.read = now;
    }
}

/// The newest set's age, as the catalog answer shows it.
#[cfg(feature = "workspaces")]
pub(crate) struct SetInfo {
    pub(crate) finished: String,
    pub(crate) age_seconds: u64,
    pub(crate) max_age_seconds: u64,
}

/// The catalog as one subject sees it: `reason` is why nothing is selectable (a condition of the
/// platform or of the subject, the same for every identity), else `selectable` holds the
/// identities it may select, sorted.
#[cfg(feature = "workspaces")]
pub(crate) struct Catalog {
    pub(crate) reason: Option<&'static str>,
    pub(crate) set: Option<SetInfo>,
    pub(crate) selectable: BTreeSet<String>,
}

#[cfg(feature = "workspaces")]
impl Catalog {
    fn pending(reason: &'static str, set: Option<SetInfo>) -> Self {
        Self {
            reason: Some(reason),
            set,
            selectable: BTreeSet::new(),
        }
    }
}

/// What a read found: no set, the set already loaded at its age and the instant it was taken,
/// or another one, loaded.
enum Read {
    None,
    Same(Duration, Instant),
    New(Box<Set>),
}

/// Why a read failed: why, and whether it is the version refusal; `None` for a set row that
/// failed its checks, its line already written.
type Unread = Option<(String, bool)>;

impl Permissions {
    /// The cache of `github` over `postgres`, empty until `poll` starts.
    pub(super) fn new(github: Github, postgres: crate::postgres::Config) -> Arc<Self> {
        Arc::new(Self {
            github,
            postgres,
            state: RwLock::new(State::default()),
        })
    }

    /// Starts the thread that reads the newest set now and every `poll_every()` while the cache
    /// lives: once the listener printed its address, so that line stays the first on stderr.
    pub(super) fn poll(self: &Arc<Self>) {
        let cache = Arc::downgrade(self);
        std::thread::spawn(move || poll(cache, poll_every()));
    }

    /// `[oidc.github] host`.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub(super) fn host(&self) -> &str {
        &self.github.host
    }

    /// The identities `login` reads through GitHub and the grant's state, both from one read
    /// of the set at one instant: `current` with the set's rows while it is at most
    /// `max_age_seconds` old, `expired` with none past it, `unread` with none while no set is
    /// loaded. Never `*` (a row's identity is on `host`).
    pub(super) fn grant_and_state(&self, login: Option<&str>) -> (Vec<String>, &'static str) {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        self.read_at(state.set.as_ref(), login, Instant::now())
    }

    /// `grant_and_state` for `set` at `now`.
    fn read_at(
        &self,
        set: Option<&Set>,
        login: Option<&str>,
        now: Instant,
    ) -> (Vec<String>, &'static str) {
        let state = self.state_of(set, now);
        let grant = match (set, login) {
            (Some(set), Some(login)) if state == "current" => {
                set.logins.get(login).cloned().unwrap_or_default()
            }
            _ => Vec::new(),
        };
        (grant, state)
    }

    fn state_of(&self, set: Option<&Set>, now: Instant) -> &'static str {
        match set {
            None => "unread",
            Some(set) if set.age(now) <= self.github.max_age => "current",
            Some(_) => "expired",
        }
    }

    /// The newest set's workspace version and the identities it excluded from that version
    /// (#333), whatever the set's age: what the last role check left out. `None` before a set is
    /// loaded or when it names no version.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub(super) fn excluded(&self) -> Option<(String, BTreeSet<String>)> {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        let set = state.set.as_ref()?;
        Some((set.version.clone()?, set.excluded.clone()))
    }

    /// The identities the newest set left out of the named workspace `context` (#342): the sponsor
    /// rule's exclusions, whatever the set's age. Empty before a set is loaded.
    #[cfg(feature = "workspaces")]
    pub(super) fn excluded_in(&self, context: &str) -> BTreeSet<String> {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        (state.set.as_ref())
            .and_then(|set| set.context_excluded.get(context))
            .cloned()
            .unwrap_or_default()
    }

    /// The admission view's `github` (#332): the grant provider's settings, the grant's state and
    /// the newest set as loaded (null before one is), its counts as the job wrote them.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub(super) fn admission(&self) -> Value {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let set = state.set.as_ref().map(|set| {
            #[allow(unused_mut)]
            let mut shown = json!({
                "id": set.id,
                "finished": set.finished,
                "age_seconds": set.age(now).as_secs(),
                "repositories": set.repositories,
                "rows": set.rows,
                "exclusions": set.exclusions,
            });
            #[cfg(feature = "workspaces")]
            if self.github.catalog {
                shown["catalog"] = json!(set.catalog.len());
            }
            shown
        });
        json!({
            "host": self.github.host,
            "login_claim": self.github.login_claim,
            "roles": self.github.roles,
            "max_age_seconds": self.github.max_age.as_secs(),
            "grant": self.state_of(state.set.as_ref(), now),
            "set": set,
        })
    }

    /// The catalog as `login` may select from it (#341, ADR 0021): `administrator` the whole
    /// catalog, anyone else the repositories its login holds an accepted role on in the newest
    /// set. Pending (nothing selectable) while no set is loaded, while the set is past
    /// `max_age_seconds` and, for anyone but the administrator, without a login. Never reads
    /// Postgres.
    #[cfg(feature = "workspaces")]
    pub(super) fn catalog(&self, login: Option<&str>, administrator: bool) -> Catalog {
        let state = self.state.read().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let Some(set) = &state.set else {
            return Catalog::pending("no_set", None);
        };
        let age = set.age(now);
        let info = Some(SetInfo {
            finished: set.finished.clone(),
            age_seconds: age.as_secs(),
            max_age_seconds: self.github.max_age.as_secs(),
        });
        if age > self.github.max_age {
            return Catalog::pending("stale_set", info);
        }
        let selectable = match (administrator, login) {
            (true, _) => set.catalog.clone(),
            (false, None) => return Catalog::pending("missing_login", info),
            (false, Some(login)) => {
                let roles = set.logins.get(login).map(Vec::as_slice).unwrap_or_default();
                roles
                    .iter()
                    .filter(|i| set.catalog.contains(*i))
                    .cloned()
                    .collect()
            }
        };
        Catalog {
            reason: None,
            set: info,
            selectable,
        }
    }

    /// One poll: the newest set read, the cache updated, or the stderr line of a failure.
    fn refresh(&self) {
        let known = {
            let state = self.state.read().unwrap_or_else(|e| e.into_inner());
            state.set.as_ref().map(|set| set.id.clone())
        };
        // The poll thread is the only reader: a Postgres that stops answering must not stop it.
        let bound = self.postgres.bound(STATEMENTS);
        let read = crate::modules::within(
            bound,
            async { Ok(self.read(known.as_deref()).await) },
            Msg::PostgresTimeout,
        );
        let read = read.unwrap_or_else(|why| Err(Some((why, false))));
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        match read {
            Ok(Read::None) => state.set = None,
            Ok(Read::Same(age, at)) => {
                if let Some(set) = &mut state.set {
                    set.seen(age, at);
                }
            }
            Ok(Read::New(set)) => state.set = Some(*set),
            Err(None) => {}
            Err(Some((why, true))) => {
                if !state.version_said {
                    eprintln!("{}", Msg::PermissionsVersion(&why).text());
                    state.version_said = true;
                }
            }
            Err(Some((why, false))) => eprintln!("{}", Msg::PermissionsUnreadable(&why).text()),
        }
    }

    /// The newest set, its age on Postgres's clock, and its rows when its id is not `known`, in
    /// one read-only transaction: one snapshot, so rows and counts are the set's own.
    async fn read(&self, known: Option<&str>) -> Result<Read, Unread> {
        let mut opened = crate::postgres::versioned(&self.postgres)
            .await
            .map_err(Some)?;
        let scrub = opened.scrubber();
        let failed = |e| Some((scrub(e), false));
        let q = self.postgres.quoted();
        let tx = (opened.client.build_transaction())
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()
            .await
            .map_err(failed)?;
        // Taken before the age is: the time the rest of the read takes counts in the age.
        let at = Instant::now();
        let newest = format!(
            "SELECT id, to_char(finished AT TIME ZONE 'UTC', \
             'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
             extract(epoch FROM clock_timestamp() - finished)::float8, repositories, version \
             FROM {q}.permission_sets ORDER BY fence DESC, finished DESC LIMIT 1"
        );
        let Some(row) = tx.query_opt(&newest, &[]).await.map_err(failed)? else {
            return Ok(Read::None);
        };
        let sets = format!("{q}.permission_sets");
        let id: Option<String> = row.try_get(0).ok();
        let finished: Option<String> = row.try_get(1).ok();
        let age = (row.try_get::<_, f64>(2).ok()).and_then(|s| Duration::try_from_secs_f64(s).ok());
        let repositories = row.try_get::<_, i32>(3).ok().filter(|n| *n >= 0);
        // A finished time ahead of Postgres's clock is no age: the set grants nothing new.
        let (Some(id), Some(finished), Some(age), Some(repositories)) = (
            id.filter(|id| set_id_ok(id)),
            finished.filter(|t| crate::postgres::is_time(t)),
            age,
            repositories,
        ) else {
            eprintln!("{}", Msg::PermissionsBadRow(&sets).text());
            return Err(None);
        };
        if known == Some(id.as_str()) {
            return Ok(Read::Same(age, at));
        }
        // The version it checked only names what the console marks `excluded`: one that is no
        // version id marks nothing, and the set still grants.
        let version = match row.try_get::<_, Option<String>>(4) {
            Ok(None) => None,
            Ok(Some(version)) if meada_core::ids::is_id(&version) => Some(version),
            _ => {
                eprintln!("{}", Msg::PermissionsBadRow(&sets).text());
                None
            }
        };
        let counts = format!(
            "SELECT (SELECT count(*) FROM {q}.permission_rows WHERE set_id = $1), \
             (SELECT count(*) FROM {q}.permission_exclusions WHERE set_id = $1)"
        );
        let counted = tx.query_one(&counts, &[&id]).await.map_err(failed)?;
        let accepted = format!(
            "SELECT identity, login, role FROM {q}.permission_rows \
             WHERE set_id = $1 AND role = ANY($2)"
        );
        let rows = (tx.query(&accepted, &[&id, &self.github.roles]).await).map_err(failed)?;
        let table = format!("{q}.permission_rows");
        let mut logins: HashMap<String, Vec<String>> = HashMap::new();
        for row in rows {
            let text = |i| row.try_get::<_, String>(i).ok();
            match (text(0), text(1), text(2)) {
                (Some(identity), Some(login), Some(role))
                    if self.identity_ok(&identity) && login_ok(&login) && role_ok(&role) =>
                {
                    logins.entry(login).or_default().push(identity);
                }
                _ => eprintln!("{}", Msg::PermissionsBadRow(&table).text()),
            }
        }
        #[cfg(feature = "workspaces")]
        let catalog = self.read_catalog(&tx, &q, &id).await.map_err(failed)?;
        let exclusions =
            format!("SELECT context, identity FROM {q}.permission_exclusions WHERE set_id = $1");
        let table = format!("{q}.permission_exclusions");
        let mut excluded = BTreeSet::new();
        #[cfg(feature = "workspaces")]
        let mut context_excluded: HashMap<String, BTreeSet<String>> = HashMap::new();
        for row in tx.query(&exclusions, &[&id]).await.map_err(failed)? {
            match (
                row.try_get::<_, String>(0).ok(),
                row.try_get::<_, String>(1).ok(),
            ) {
                (Some(context), Some(identity)) if self.identity_ok(&identity) => {
                    if context.is_empty() {
                        excluded.insert(identity);
                    } else if cfg!(feature = "workspaces") && meada_core::ids::is_id(&context) {
                        #[cfg(feature = "workspaces")]
                        context_excluded
                            .entry(context)
                            .or_default()
                            .insert(identity);
                    } else {
                        eprintln!("{}", Msg::PermissionsBadRow(&table).text());
                    }
                }
                _ => eprintln!("{}", Msg::PermissionsBadRow(&table).text()),
            }
        }
        Ok(Read::New(Box::new(Set {
            id,
            finished,
            age,
            read: at,
            repositories: repositories.into(),
            rows: counted.get(0),
            exclusions: counted.get(1),
            logins,
            version,
            excluded,
            #[cfg(feature = "workspaces")]
            context_excluded,
            #[cfg(feature = "workspaces")]
            catalog,
        })))
    }

    /// The identities of the set's catalog, with `[catalog]` only: its `catalog` and `both`
    /// repositories whose coverage is `listed`. A row that fails its checks is skipped with one
    /// line naming its table.
    #[cfg(feature = "workspaces")]
    async fn read_catalog(
        &self,
        tx: &tokio_postgres::Transaction<'_>,
        q: &str,
        id: &str,
    ) -> Result<BTreeSet<String>, tokio_postgres::Error> {
        let mut catalog = BTreeSet::new();
        if !self.github.catalog {
            return Ok(catalog);
        }
        let listed = format!(
            "SELECT identity FROM {q}.permission_repositories \
             WHERE set_id = $1 AND source IN ('catalog', 'both') AND coverage = 'listed'"
        );
        let table = format!("{q}.permission_repositories");
        for row in tx.query(&listed, &[&id]).await? {
            match row.try_get::<_, String>(0).ok() {
                Some(identity)
                    if self.identity_ok(&identity) && identity.len() <= CATALOG_IDENTITY_MAX =>
                {
                    catalog.insert(identity);
                }
                _ => eprintln!("{}", Msg::PermissionsBadRow(&table).text()),
            }
        }
        Ok(catalog)
    }

    /// Whether a row's identity may enter a grant: on `host`, an identity a served repository
    /// may have, with no control character or space.
    fn identity_ok(&self, identity: &str) -> bool {
        (identity.strip_prefix(&self.github.host)).is_some_and(|rest| rest.starts_with('/'))
            && meada_core::manifest::identity_problem(identity).is_none()
            && !identity
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
    }
}

/// Whether `id` is a set's id: 32 lowercase hex characters.
fn set_id_ok(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether a row's login is one the job writes: GitHub's, lowercased.
fn login_ok(login: &str) -> bool {
    meada_core::manifest::added_by_problem(login).is_none()
}

/// A token's login: `login_claim`'s value when it is a string of 1 to 39 ASCII letters, digits,
/// `-` and `_`, lowercased, as GitHub compares logins; anything else is none.
pub(super) fn login(claim: Option<&Value>) -> Option<String> {
    let login = claim?.as_str()?.to_ascii_lowercase();
    login_ok(&login).then_some(login)
}

/// The poll thread: reads the newest set of `cache` now and every `every`, until it is gone.
fn poll(cache: Weak<Permissions>, every: Duration) {
    loop {
        let Some(cache) = cache.upgrade() else {
            return;
        };
        cache.refresh();
        drop(cache);
        std::thread::sleep(every);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn github(max_age: u64) -> Github {
        Github {
            host: "github.com".into(),
            login_claim: "login".into(),
            roles: vec!["write".into()],
            max_age: Duration::from_secs(max_age),
            postgres: None,
            #[cfg(feature = "workspaces")]
            catalog: false,
        }
    }

    fn cache(max_age: u64) -> Permissions {
        let toml = "url_env = \"MEADA_TEST_UNSET\"";
        let file: crate::postgres::File = toml::from_str(toml).unwrap();
        Permissions {
            github: github(max_age),
            postgres: file.validate(std::path::Path::new(".")).unwrap(),
            state: RwLock::new(State::default()),
        }
    }

    /// A set read at `read` with `age` on Postgres's clock, granting `ana` one repository.
    fn set(age: u64, read: Instant) -> Set {
        Set {
            id: "0".repeat(32),
            finished: "2026-10-29T00:00:00.000000Z".into(),
            age: Duration::from_secs(age),
            read,
            repositories: 1,
            rows: 1,
            exclusions: 0,
            logins: HashMap::from([("ana".into(), vec!["github.com/acme/api".into()])]),
            version: None,
            excluded: BTreeSet::new(),
            #[cfg(feature = "workspaces")]
            context_excluded: HashMap::new(),
            #[cfg(feature = "workspaces")]
            catalog: BTreeSet::new(),
        }
    }

    #[test]
    fn a_set_grants_up_to_its_max_age_exactly_then_expires() {
        let cache = cache(60);
        let now = Instant::now();
        assert_eq!(cache.state_of(None, now), "unread");
        assert!(cache.grant_and_state(Some("ana")).0.is_empty());
        let fresh = set(60, now);
        assert_eq!(cache.state_of(Some(&fresh), now), "current");
        let later = now + Duration::from_millis(1);
        assert_eq!(cache.state_of(Some(&fresh), later), "expired");
        cache.state.write().unwrap().set = Some(set(59, Instant::now()));
        let (rows, state) = cache.grant_and_state(Some("ana"));
        assert_eq!(
            (rows.as_slice(), state),
            (&["github.com/acme/api".to_string()][..], "current")
        );
        assert!(cache.grant_and_state(Some("bia")).0.is_empty());
        cache.state.write().unwrap().set = Some(set(61, Instant::now()));
        assert_eq!(cache.grant_and_state(Some("ana")), (Vec::new(), "expired"));
    }

    #[test]
    fn the_grant_and_its_state_never_disagree_across_the_max_age_boundary() {
        let cache = cache(60);
        let now = Instant::now();
        let fresh = set(60, now);
        let at = cache.read_at(Some(&fresh), Some("ana"), now);
        assert_eq!(at, (vec!["github.com/acme/api".to_string()], "current"));
        let past = now + Duration::from_millis(1);
        let after = cache.read_at(Some(&fresh), Some("ana"), past);
        assert_eq!(after, (Vec::new(), "expired"));
        assert_eq!(
            cache.read_at(None, Some("ana"), now),
            (Vec::new(), "unread")
        );
        assert_eq!(
            cache.read_at(Some(&fresh), None, now),
            (Vec::new(), "current")
        );
    }

    #[test]
    fn a_set_keeps_aging_and_a_clock_moved_back_never_makes_it_younger() {
        let start = Instant::now();
        let mut held = set(10, start);
        let later = start + Duration::from_secs(30);
        assert_eq!(held.age(later), Duration::from_secs(40));
        // Postgres's clock moved back: the same set read again is not younger.
        held.seen(Duration::from_secs(5), later);
        assert_eq!(held.age(later), Duration::from_secs(40));
        // Moved forward, it is older.
        held.seen(Duration::from_secs(100), later);
        assert_eq!(
            held.age(later + Duration::from_secs(1)),
            Duration::from_secs(101)
        );
        // The process's clock never runs back either.
        assert_eq!(held.age(start), Duration::from_secs(100));
    }

    #[test]
    fn a_login_is_a_short_ascii_string_compared_in_lowercase() {
        let login = |v: Value| super::login(Some(&v));
        assert_eq!(login(json!("Ana-Souza_2")).as_deref(), Some("ana-souza_2"));
        assert_eq!(login(json!("a".repeat(39))), Some("a".repeat(39)));
        for refused in [
            json!(""),
            json!("a".repeat(40)),
            json!("ana.souza"),
            json!("ana souza"),
            json!("ánа"),
            json!(42),
            json!(["ana"]),
            json!(null),
        ] {
            assert_eq!(login(refused.clone()), None, "{refused}");
        }
        assert_eq!(super::login(None), None);
    }

    #[test]
    fn a_row_must_be_on_the_host_with_a_safe_identity() {
        let cache = cache(60);
        assert!(cache.identity_ok("github.com/acme/api"));
        for refused in [
            "*",
            "github.com",
            "github.comx/acme/api",
            "gitlab.com/acme/api",
            "github.com/../etc",
            "github.com/acme/a pi",
            "github.com/acme/api\n",
        ] {
            assert!(!cache.identity_ok(refused), "{refused:?}");
        }
        assert!(set_id_ok(&"ab".repeat(16)));
        assert!(!set_id_ok(&"AB".repeat(16)) && !set_id_ok("abc"));
    }

    /// The catalog against the shared Y/A/B table (`tests/fixtures/named-workspaces.json`,
    /// ADR 0020): a set whose rows are the table's accepted ones and whose catalog holds Y, A and
    /// B answers each caller's creation rows alike, at a fresh age and past `max_age_seconds`.
    #[cfg(feature = "workspaces")]
    #[test]
    fn the_catalog_follows_the_shared_table() {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct Table {
            identities: HashMap<String, String>,
            accepted_roles: Vec<String>,
            callers: HashMap<String, Caller>,
            creation: Vec<Case>,
            stale: Vec<Case>,
        }
        #[derive(Deserialize)]
        struct Caller {
            login: Option<String>,
            rows: Vec<Row>,
            #[serde(default)]
            platform_administrator: bool,
        }
        #[derive(Deserialize)]
        struct Row {
            identity: String,
            role: String,
        }
        #[derive(Deserialize)]
        struct Case {
            caller: String,
            select: Vec<String>,
            allowed: bool,
            pending: Option<String>,
        }
        let table: Table =
            serde_json::from_str(include_str!("../../tests/fixtures/named-workspaces.json"))
                .unwrap();
        let mut github = github(60);
        github.roles = table.accepted_roles.clone();
        github.catalog = true;
        let mut cache = cache(60);
        cache.github = github;
        let mut fresh = set(0, Instant::now());
        fresh.logins.clear();
        for caller in table.callers.values() {
            let Some(login) = &caller.login else { continue };
            for row in &caller.rows {
                if table.accepted_roles.contains(&row.role) {
                    let identity = table.identities[&row.identity].clone();
                    fresh
                        .logins
                        .entry(login.clone())
                        .or_default()
                        .push(identity);
                }
            }
        }
        fresh.catalog = table.identities.values().cloned().collect();
        let mut stale = set(61, Instant::now());
        stale.logins = fresh.logins.clone();
        stale.catalog = fresh.catalog.clone();

        for (set, cases) in [(fresh, &table.creation), (stale, &table.stale)] {
            cache.state.write().unwrap().set = Some(set);
            for case in cases {
                let caller = &table.callers[&case.caller];
                let seen = cache.catalog(caller.login.as_deref(), caller.platform_administrator);
                let name = format!("{} selects {:?}", case.caller, case.select);
                if let Some(pending) = &case.pending {
                    assert_eq!(seen.reason, Some(pending.as_str()), "{name}");
                    assert!(seen.selectable.is_empty(), "{name}");
                    continue;
                }
                let wanted = case.select.iter().map(|i| &table.identities[i]);
                let all = wanted.clone().all(|i| seen.selectable.contains(i));
                assert_eq!((seen.reason, all), (None, case.allowed), "{name}");
                // Nothing beyond the caller's accepted rows, or the whole catalog for the
                // administrator, is ever selectable.
                let rows = caller
                    .rows
                    .iter()
                    .filter(|r| table.accepted_roles.contains(&r.role))
                    .map(|r| table.identities[&r.identity].clone());
                let expected: BTreeSet<String> = match caller.platform_administrator {
                    true => table.identities.values().cloned().collect(),
                    false => rows.collect(),
                };
                assert_eq!(seen.selectable, expected, "{name}");
            }
        }
    }

    /// Without a set the catalog is pending `no_set`; a row outside the catalog is not
    /// selectable though its login holds a role on it (coverage is the catalog's, the role is
    /// the login's, and only both select).
    #[cfg(feature = "workspaces")]
    #[test]
    fn the_catalog_needs_a_set_and_both_coverage_and_a_role() {
        let mut cache = cache(60);
        cache.github.catalog = true;
        assert_eq!(cache.catalog(Some("ana"), false).reason, Some("no_set"));
        let mut loaded = set(0, Instant::now());
        loaded.catalog = ["github.com/acme/other".to_string()].into();
        cache.state.write().unwrap().set = Some(loaded);
        let seen = cache.catalog(Some("ana"), false);
        assert_eq!((seen.reason, seen.selectable.len()), (None, 0));
        assert_eq!(cache.catalog(None, false).reason, Some("missing_login"));
        assert_eq!(cache.catalog(None, true).selectable.len(), 1);
    }
}
