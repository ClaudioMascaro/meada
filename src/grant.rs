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

pub(crate) fn role_ok(role: &str) -> bool {
    (1..=ROLE_MAX).contains(&role.chars().count()) && !role.chars().any(char::is_control)
}

#[cfg(feature = "accounts")]
pub fn login_ok(login: &str) -> bool {
    permissions::login(Some(login)).as_deref() == Some(login)
}

/// What an admitted subject may read.
#[derive(Clone)]
pub enum Grant {
    /// `*` is among its identities.
    All,
    Only(HashSet<String>),
}

#[cfg(feature = "snapshot")]
impl From<&Grant> for meada_core::authorize::Grant {
    fn from(grant: &Grant) -> Self {
        match grant {
            Grant::All => Self::All,
            Grant::Only(identities) => Self::Only(identities.iter().cloned().collect()),
        }
    }
}

impl Grant {
    pub fn reads(&self, identity: &str) -> bool {
        match self {
            Grant::All => true,
            Grant::Only(identities) => identities.contains(identity),
        }
    }

    fn of<'a>(identities: impl IntoIterator<Item = &'a str>) -> Self {
        let listed: HashSet<String> = identities.into_iter().map(str::to_string).collect();
        match listed.contains("*") {
            true => Grant::All,
            false => Grant::Only(listed),
        }
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(not(feature = "postgres"), expect(dead_code))]
pub enum GrantState {
    Unread,
    Current,
    Expired,
}

pub struct GithubGrant {
    pub identities: Vec<String>,
    #[cfg_attr(
        not(any(feature = "ui", all(test, feature = "postgres"))),
        expect(dead_code)
    )]
    pub state: GrantState,
    #[cfg_attr(
        not(any(feature = "ui", all(test, feature = "postgres"))),
        expect(dead_code)
    )]
    pub host: String,
}

pub struct Granted {
    pub claims: Grant,
    #[cfg_attr(not(any(feature = "ui", test)), expect(dead_code))]
    pub login: Option<String>,
    pub github: Option<GithubGrant>,
}

impl Granted {
    pub fn union(&self) -> Grant {
        let github = self.github.as_ref().map(|g| g.identities.as_slice());
        self.claims.with(github.unwrap_or_default())
    }

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

pub struct Providers {
    groups: Option<HashMap<String, Vec<String>>>,
    #[cfg(feature = "postgres")]
    github: Option<Arc<permissions::Permissions>>,
}

impl Providers {
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

    pub fn of(manifest: &crate::workspace::Manifest) -> Self {
        let groups = (manifest.oidc.as_ref())
            .and_then(|oidc| oidc.groups.as_ref())
            .map(|(_, table)| table.clone());
        #[cfg(feature = "accounts")]
        let groups = groups.or_else(|| {
            let accounts = manifest.accounts.as_ref()?;
            accounts.groups.clone()
        });
        Self::new(groups, manifest.github())
    }

    pub fn listening(&self) {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            cache.poll();
        }
    }

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

    #[cfg(feature = "accounts")]
    pub fn local(&self, groups: &[String], login: Option<&str>) -> Granted {
        match self.bounds() {
            true => self.grant(groups, login),
            false => Granted {
                claims: Grant::All,
                login: None,
                github: None,
            },
        }
    }

    #[cfg(feature = "accounts")]
    pub fn bounds(&self) -> bool {
        self.groups.is_some() || self.github().is_some()
    }

    #[cfg_attr(not(any(feature = "ui", test)), expect(dead_code))]
    pub fn github(&self) -> Option<&str> {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            return Some(cache.host());
        }
        None
    }

    #[cfg(feature = "workspaces")]
    pub fn catalog(&self, login: Option<&str>, administrator: bool) -> Option<Catalog> {
        (self.github.as_ref()).map(|cache| cache.catalog(login, administrator))
    }

    #[cfg(feature = "workspaces")]
    pub fn excluded_in(&self, context: &str) -> std::collections::BTreeSet<String> {
        (self.github.as_ref())
            .map(|cache| cache.excluded_in(context))
            .unwrap_or_default()
    }

    #[cfg(feature = "ui")]
    #[cfg_attr(not(feature = "generation-pointer"), expect(dead_code))]
    pub fn excluded(&self) -> Option<(String, std::collections::BTreeSet<String>)> {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            return cache.excluded();
        }
        None
    }

    #[cfg(feature = "ui")]
    pub fn admission(&self) -> Option<serde_json::Value> {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.github {
            return Some(cache.admission());
        }
        None
    }
}

#[cfg(feature = "ui")]
pub fn bounded(
    table: &HashMap<String, Vec<String>>,
    reads: &dyn Fn(&str) -> bool,
) -> serde_json::Map<String, serde_json::Value> {
    (table.iter())
        .map(|(group, identities)| {
            let shown = identities.iter().filter(|i| *i == "*" || reads(i));
            (group.clone(), shown.cloned().collect::<Vec<_>>().into())
        })
        .collect()
}

#[cfg(feature = "ui")]
pub struct Admitted {
    pub subject: Option<String>,
    /// The global operator.
    pub operator: bool,
    pub groups: Vec<String>,
    pub login: Option<String>,
    pub grant: Grant,
    pub github: Option<(String, GrantState)>,
}

#[cfg(feature = "ui")]
impl Admitted {
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
        assert_eq!(grant(&["g", "p"]), None);
        assert_eq!(grant(&["p"]), None);
        assert_eq!(grant(&["unmapped"]), only(&[]));
        assert_eq!(grant(&["unmapped", "g"]), only(&["b"]));
        assert_eq!(grant(&[]), only(&[]));
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
