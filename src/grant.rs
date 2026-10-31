//! The grant providers (ADR 0016, feature `oidc`), fed by neutral inputs: what a subject may read
//! is computed from its groups and its GitHub login, whichever verifier admitted it. The
//! identity provider (`oidc`) is one producer of those inputs: it reads them from a token's
//! claims and hands them to `Providers::grant`.
//!
//! `Providers` holds what a deployment configures: the groups table (a group to the identities it
//! grants, `*` for all; `[oidc.groups]`) and the GitHub grant provider (`Github`, reached through
//! `Manifest::github`) with its permissions cache (`permissions`, feature `postgres`). One
//! `Providers` serves a process: the node's gate, the console and the proposal server each build
//! it once from their manifest (`Providers::of`) and hand it to their verifier.
//!
//! `Providers::grant` is the grant: the groups side (what the table maps the groups to) and, with
//! the GitHub grant provider, the GitHub side (the identities the login holds an accepted role on
//! in the newest permissions set, while it is at most `max_age_seconds` old, never `*`) and its
//! state, both read at one instant. `Granted::union` is the grant every check reads. `Admitted`
//! is the neutral admitted subject the console and the proposal server build their sessions from.

use std::collections::{HashMap, HashSet};
#[cfg(feature = "postgres")]
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "postgres")]
mod permissions;
#[cfg(feature = "workspaces")]
pub(crate) use permissions::Catalog;
#[cfg(all(debug_assertions, feature = "postgres"))]
pub(crate) use permissions::poll_knob;

// ponytail: the longest role name the GitHub grant provider's `roles` takes, in characters, and
// the longest `role_name` from GitHub the role check stores; a longer one is refused (a skipped
// row).
pub(crate) const ROLE_MAX: usize = 100;

/// Whether `role` is a role name the GitHub grant provider's `roles` takes: 1 to `ROLE_MAX`
/// characters with no control character. GitHub's `role_name`s are checked alike before they are
/// stored.
pub(crate) fn role_ok(role: &str) -> bool {
    (1..=ROLE_MAX).contains(&role.chars().count()) && !role.chars().any(char::is_control)
}

/// What an admitted subject may read.
#[derive(Clone)]
pub enum Grant {
    /// `*` is among its identities.
    All,
    Only(HashSet<String>),
}

impl Grant {
    pub fn reads(&self, identity: &str) -> bool {
        match self {
            Grant::All => true,
            Grant::Only(identities) => identities.contains(identity),
        }
    }

    /// The grant as `meada-core` takes it, for the rules the console and a node share.
    #[cfg(feature = "snapshot")]
    pub fn core(&self) -> meada_core::authorize::Grant {
        match self {
            Grant::All => meada_core::authorize::Grant::All,
            Grant::Only(identities) => {
                meada_core::authorize::Grant::Only(identities.iter().cloned().collect())
            }
        }
    }

    /// The grant of `identities`: all when one is `*`.
    fn of<'a>(identities: impl IntoIterator<Item = &'a str>) -> Self {
        let listed: HashSet<String> = identities.into_iter().map(str::to_string).collect();
        match listed.contains("*") {
            true => Grant::All,
            false => Grant::Only(listed),
        }
    }

    /// The union with `more` (ADR 0016): `*` stays all.
    fn with(&self, more: &[String]) -> Self {
        match self {
            Grant::All => Grant::All,
            Grant::Only(identities) => {
                Grant::Only(identities.iter().chain(more).cloned().collect())
            }
        }
    }
}

/// The GitHub grant provider's settings (ADR 0016), `[oidc.github]` as validated: what `meada
/// github-app roles` checks against (#331) and the grant at admission reads (#332). Every reader
/// but `Manifest::read`, which fills it, reaches it through `Manifest::github`.
#[derive(Clone, Debug, PartialEq)]
pub struct Github {
    /// The git host whose identities it grants, a lowercase host name.
    pub host: String,
    /// The accepted roles, compared exactly with GitHub's `role_name`.
    pub roles: Vec<String>,
    /// How old the newest permissions set may be and still grant.
    pub max_age: Duration,
    /// The manifest's `[postgres]`, where the set is read: `Manifest::read` sets it, and
    /// refuses the GitHub grant provider without it.
    #[cfg(feature = "postgres")]
    pub postgres: Option<crate::postgres::Config>,
    /// Whether the manifest has `[catalog]` (#341): `Manifest::read` sets it, and the permissions
    /// cache then reads the newest set's catalog beside its rows.
    #[cfg(feature = "workspaces")]
    pub catalog: bool,
}

/// The GitHub side of a grant: the identities the login reads through GitHub, the grant's state
/// read with them (`unread`, `current` or `expired`) and the GitHub grant provider's host.
pub struct GithubGrant {
    pub identities: Vec<String>,
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub state: &'static str,
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub host: String,
}

/// A subject's grant by side (ADR 0016).
pub struct Granted {
    /// The groups side (with an identity provider, the claims grant): what the groups table maps
    /// the subject's groups to, and the identities its verifier grants by name (`listing`).
    pub claims: Grant,
    /// With the GitHub grant provider, the subject's login when it is a valid one.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub login: Option<String>,
    /// With the GitHub grant provider, its grant (empty without a valid login or a current set).
    pub github: Option<GithubGrant>,
}

impl Granted {
    /// The grant every check reads: the union of both sides.
    pub fn union(&self) -> Grant {
        let github = self.github.as_ref().map(|g| g.identities.as_slice());
        self.claims.with(github.unwrap_or_default())
    }

    /// This grant with `identities` also granted on the groups side, `*` for all: what a
    /// verifier grants by name (the identity provider's `identities_claim`).
    pub fn listing<'a>(mut self, identities: impl IntoIterator<Item = &'a str>) -> Self {
        self.claims = match (self.claims, Grant::of(identities)) {
            (Grant::Only(mut mine), Grant::Only(more)) => {
                mine.extend(more);
                Grant::Only(mine)
            }
            _ => Grant::All,
        };
        self
    }
}

/// The grant providers of a process: the groups table and the GitHub grant provider with its
/// permissions cache.
pub struct Providers {
    /// A group to the identities it grants.
    groups: Option<HashMap<String, Vec<String>>>,
    /// The permissions cache of the GitHub grant provider (#332).
    #[cfg(feature = "postgres")]
    github: Option<Arc<permissions::Permissions>>,
}

impl Providers {
    /// The grant providers of the groups table `groups` and the GitHub grant provider `github`:
    /// its permissions cache over `github`'s `[postgres]`, empty until `listening`.
    pub fn new(groups: Option<HashMap<String, Vec<String>>>, github: Option<&Github>) -> Self {
        #[cfg(feature = "postgres")]
        let github = github
            .and_then(|github| Some((github.postgres.clone()?, github.clone())))
            .map(|(postgres, github)| permissions::Permissions::new(github, postgres));
        #[cfg(not(feature = "postgres"))]
        let _ = github;
        Self {
            groups,
            #[cfg(feature = "postgres")]
            github,
        }
    }

    /// The grant providers `manifest` configures: `[oidc.groups]` and `Manifest::github`.
    pub fn of(manifest: &crate::workspace::Manifest) -> Self {
        let groups = (manifest.oidc.as_ref())
            .and_then(|oidc| oidc.groups.as_ref())
            .map(|(_, table)| table.clone());
        Self::new(groups, manifest.github())
    }

    /// Starts reading the permissions set (#332), which admission never waits on: a listener
    /// calls it once, after its address is the first line on stderr.
    pub fn listening(&self) {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            cache.poll();
        }
    }

    /// The grant of a subject with `groups` and `login`, whichever verifier admitted it: the
    /// identities the groups table maps `groups` to (a group it lacks grants nothing, `*` all)
    /// and, with the GitHub grant provider, the GitHub grant of `login` when it is a valid one
    /// (1 to 39 ASCII letters, digits, `-` and `_`, compared lowercased), read from the cache,
    /// never from Postgres.
    pub fn grant(&self, groups: &[String], login: Option<&str>) -> Granted {
        let mapped = (self.groups.iter()).flat_map(|table| {
            (groups.iter().filter_map(|group| table.get(group)))
                .flatten()
                .map(String::as_str)
        });
        let claims = Grant::of(mapped);
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            let login = permissions::login(login);
            let (identities, state) = cache.grant_and_state(login.as_deref());
            let host = cache.host().to_string();
            return Granted {
                claims,
                login,
                github: Some(GithubGrant {
                    identities,
                    state,
                    host,
                }),
            };
        }
        let _ = login;
        Granted {
            claims,
            login: None,
            github: None,
        }
    }

    /// With the GitHub grant provider, its host, whatever a subject's login (#332).
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub fn github(&self) -> Option<&str> {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            return Some(cache.host());
        }
        None
    }

    /// With the GitHub grant provider, the catalog as `login` may select from it (#341): the
    /// whole fresh catalog for the platform administrator. `None` without the permissions cache.
    #[cfg(feature = "workspaces")]
    pub fn catalog(&self, login: Option<&str>, administrator: bool) -> Option<Catalog> {
        (self.github.as_ref()).map(|cache| cache.catalog(login, administrator))
    }

    /// With the GitHub grant provider, the identities the newest set left out of the named
    /// workspace `context` (#342).
    #[cfg(feature = "workspaces")]
    pub fn excluded_in(&self, context: &str) -> std::collections::BTreeSet<String> {
        (self.github.as_ref())
            .map(|cache| cache.excluded_in(context))
            .unwrap_or_default()
    }

    /// With the GitHub grant provider, the newest permissions set's workspace version and the
    /// identities it excluded from it (#333), for the console's `excluded` flag.
    #[cfg(feature = "ui")]
    #[cfg_attr(not(feature = "generation-pointer"), allow(dead_code))]
    pub fn excluded(&self) -> Option<(String, std::collections::BTreeSet<String>)> {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            return cache.excluded();
        }
        None
    }

    /// With the GitHub grant provider, the admission view's `github` (#332): its settings, the
    /// grant's state and the newest set as loaded.
    #[cfg(feature = "ui")]
    pub fn admission(&self) -> Option<serde_json::Value> {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            return Some(cache.admission());
        }
        None
    }
}

/// A subject a verifier admitted, whichever verifier it was: what the console's session and the
/// proposal server's are built from (ADR 0014, ADR 0022).
#[cfg(feature = "ui")]
pub struct Admitted {
    /// Who it is, when the verifier names it.
    pub subject: Option<String>,
    /// The global operator.
    pub operator: bool,
    /// The groups its grant read, sorted, without repeats.
    pub groups: Vec<String>,
    /// With the GitHub grant provider, its login when it has a valid one (#332).
    pub login: Option<String>,
    /// The union of the grant providers' grants.
    pub grant: Grant,
    /// With the GitHub grant provider, its host and the GitHub grant's state read with the grant
    /// itself, so a session never pairs a grant with another instant's state.
    pub github: Option<(String, &'static str)>,
}

#[cfg(feature = "ui")]
impl Admitted {
    /// `subject`, global operator when `operator`, whose grant the grant providers computed
    /// from `groups` (`granted`).
    pub fn new(
        subject: Option<String>,
        operator: bool,
        mut groups: Vec<String>,
        granted: Granted,
    ) -> Self {
        groups.sort();
        groups.dedup();
        let grant = granted.union();
        Admitted {
            subject,
            operator,
            groups,
            login: granted.login,
            grant,
            github: granted.github.map(|github| (github.host, github.state)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identities of `grant`, sorted; `None` for all.
    pub(super) fn sorted(grant: &Grant) -> Option<Vec<String>> {
        match grant {
            Grant::All => None,
            Grant::Only(identities) => {
                let mut identities: Vec<String> = identities.iter().cloned().collect();
                identities.sort();
                Some(identities)
            }
        }
    }

    pub(super) fn only(identities: &[&str]) -> Option<Vec<String>> {
        Some(identities.iter().map(|s| s.to_string()).collect())
    }

    /// A groups table mapping `g` to `b`, `h` to `b` and `c`, and `p` to `*`.
    pub(super) fn table() -> HashMap<String, Vec<String>> {
        let identities = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
        HashMap::from([
            ("g".to_string(), identities(&["b"])),
            ("h".to_string(), identities(&["b", "c"])),
            ("p".to_string(), identities(&["*"])),
        ])
    }

    pub(super) fn groups(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_groups_side_is_the_union_of_what_the_table_maps_each_group_to() {
        let providers = Providers::new(Some(table()), None);
        let grant = |names: &[&str]| sorted(&providers.grant(&groups(names), None).union());
        assert_eq!(grant(&["g"]), only(&["b"]));
        assert_eq!(grant(&["g", "h"]), only(&["b", "c"]));
        // `*` from any group is all, whatever the others map to.
        assert_eq!(grant(&["g", "p"]), None);
        assert_eq!(grant(&["p"]), None);
        // A group the table lacks grants nothing; no group grants nothing.
        assert_eq!(grant(&["unmapped"]), only(&[]));
        assert_eq!(grant(&["unmapped", "g"]), only(&["b"]));
        assert_eq!(grant(&[]), only(&[]));
        // Without a table no group grants anything.
        let bare = Providers::new(None, None);
        assert_eq!(
            sorted(&bare.grant(&groups(&["g"]), None).union()),
            only(&[])
        );
    }

    #[test]
    fn without_the_github_grant_provider_a_login_is_neither_kept_nor_granted() {
        let providers = Providers::new(Some(table()), None);
        let granted = providers.grant(&groups(&["g"]), Some("ana"));
        assert_eq!(granted.login, None);
        assert!(granted.github.is_none());
        assert_eq!(providers.github(), None);
    }

    #[test]
    fn identities_granted_by_name_join_the_groups_side() {
        let providers = Providers::new(Some(table()), None);
        let listed = providers.grant(&groups(&["g"]), None).listing(["a"]);
        assert_eq!(sorted(&listed.claims), only(&["a", "b"]));
        let all = providers.grant(&groups(&["g"]), None).listing(["*"]);
        assert_eq!(sorted(&all.union()), None);
        let from_groups = providers.grant(&groups(&["p"]), None).listing(["a"]);
        assert_eq!(sorted(&from_groups.claims), None);
    }
}
