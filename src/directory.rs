//! The directory (ADR 0016, feature `directory`, #334): users and groups the identity provider
//! pushes into the data layer through a SCIM 2.0 endpoint on the console's listener
//! (`scim`, with `[ui]`). The bootstrap manifest's `[directory]` names, without a secret, where
//! the endpoint's bearer token is (`token_env`, `MEADA_SCIM_TOKEN` by default, or `token_file`),
//! the token claim that names a user (`user_claim`), the user attribute it matches
//! (`user_attribute`, `userName` or `externalId`) and the group attribute that names a group
//! (`group_attribute`, `displayName` by default, or `externalId`). It needs `[postgres]` and
//! `[oidc]`. Only the console reads the token, once at start: rotating it means restarting the
//! console. Nodes read the section only for overage (#336), never the token. The console reads
//! the directory (`read`, #335) at its admission, for the owner picker, `owner_missing` and the
//! admission counts; overage is S4 ticket 10.

use std::path::Path;

use serde::Deserialize;

use crate::modules::{Problem, Secret};
use crate::msg::Msg;

#[cfg(feature = "ui")]
pub mod read;
#[cfg(feature = "ui")]
pub mod scim;

/// The variable `token_env` names by default.
pub const TOKEN_ENV: &str = "MEADA_SCIM_TOKEN";
/// The fewest characters a SCIM token may have.
const TOKEN_MIN: usize = 32;
// ponytail: the most `token_file` may hold; past it the console refuses to start, naming the file.
const TOKEN_MAX: u64 = 4096;

/// The user attributes `user_attribute` accepts.
const USER_ATTRIBUTES: [&str; 2] = ["userName", "externalId"];
/// The group attributes `group_attribute` accepts.
const GROUP_ATTRIBUTES: [&str; 2] = ["displayName", "externalId"];

/// The `[directory]` section as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    token_env: Option<String>,
    token_file: Option<String>,
    user_claim: Option<String>,
    /// Any value, so that one of another type is the error naming the accepted ones.
    user_attribute: Option<toml::Value>,
    group_attribute: Option<toml::Value>,
}

/// The validated section. It names where the token is, never the token.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    token: Secret,
    /// The token claim naming a user: the console's deprovisioning (#335) and the overage (S4
    /// ticket 10) read it.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub user_claim: String,
    /// `userName` or `externalId`: what `user_claim` matches, and what a revocation marker is
    /// cleared by.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub user_attribute: &'static str,
    /// `displayName` or `externalId`: what names a group, unique among them.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub group_attribute: &'static str,
}

impl File {
    /// The section checked; `token_file` is relative to `base`, the manifest's directory.
    pub fn validate(self, base: &Path) -> Result<Config, Problem> {
        let both = || ("directory.token_file", Msg::DirectoryBoth.text());
        let token = Secret::named(self.token_env, self.token_file, base, both)?;
        let user_claim = match self.user_claim {
            None => return Err(("directory", Msg::KeyMissing("directory.user_claim").text())),
            Some(claim) if claim.is_empty() => {
                return Err(("directory.user_claim", Msg::OidcEmpty.text()));
            }
            Some(claim) => claim,
        };
        let one_of = |key, value: Option<&toml::Value>, accepted: &'static [&'static str; 2]| {
            let text = value.and_then(toml::Value::as_str);
            let found = (accepted.iter().copied()).find(|&a| text == Some(a));
            found.ok_or((key, Msg::OidcOneOf(accepted).text()))
        };
        let user_attribute = match &self.user_attribute {
            None => {
                let missing = Msg::KeyMissing("directory.user_attribute").text();
                return Err(("directory", missing));
            }
            value => one_of("directory.user_attribute", value.as_ref(), &USER_ATTRIBUTES)?,
        };
        let group_attribute = match &self.group_attribute {
            None => GROUP_ATTRIBUTES[0],
            value => one_of(
                "directory.group_attribute",
                value.as_ref(),
                &GROUP_ATTRIBUTES,
            )?,
        };
        Ok(Config {
            token: token.unwrap_or_else(|| Secret::Env(TOKEN_ENV.into())),
            user_claim,
            user_attribute,
            group_attribute,
        })
    }
}

impl Config {
    /// The SCIM token, read now (the console's start): unset, unreadable or shorter than
    /// `TOKEN_MIN` characters is the problem naming the key, never the value.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    pub fn token(&self) -> Result<String, Problem> {
        let key = match self.token {
            Secret::Env(_) => "directory.token_env",
            Secret::File(_) => "directory.token_file",
        };
        match (self.token.read(TOKEN_MAX), &self.token) {
            (Ok(Some(token)), _) if token.chars().count() >= TOKEN_MIN => Ok(token),
            (Ok(Some(_)), _) => Err((key, Msg::DirectoryTokenShort(TOKEN_MIN).text())),
            (Ok(None), Secret::Env(var)) => Err((key, Msg::PostgresUrlUnset(var).text())),
            (Ok(None), Secret::File(file)) => Err((key, file.display().to_string())),
            (Err(problem), _) => Err((key, problem)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validated(src: &str) -> Result<Config, Problem> {
        toml::from_str::<File>(src)
            .unwrap()
            .validate(Path::new("ops"))
    }

    #[test]
    fn defaults_and_accepted_values() {
        let config = validated("user_claim = \"oid\"\nuser_attribute = \"externalId\"\n").unwrap();
        assert_eq!(config.token, Secret::Env(TOKEN_ENV.into()));
        assert_eq!(
            (config.user_attribute, config.group_attribute),
            ("externalId", "displayName")
        );
        let src = "token_file = \"scim\"\nuser_claim = \"sub\"\nuser_attribute = \"userName\"\n\
                   group_attribute = \"externalId\"\n";
        let config = validated(src).unwrap();
        assert_eq!(config.token, Secret::File(Path::new("ops/scim").into()));
        assert_eq!(config.group_attribute, "externalId");
    }

    #[test]
    fn each_problem_names_its_key() {
        let base = "user_claim = \"oid\"\nuser_attribute = \"userName\"\n";
        for (src, key) in [
            (
                format!("token_env = \"A\"\ntoken_file = \"b\"\n{base}"),
                "directory.token_file",
            ),
            ("user_attribute = \"userName\"\n".into(), "directory"),
            ("user_claim = \"oid\"\n".into(), "directory"),
            (
                format!("{base}user_claim = \"\"\n").replacen("user_claim = \"oid\"\n", "", 1),
                "directory.user_claim",
            ),
            (
                base.replace("userName", "email"),
                "directory.user_attribute",
            ),
            (
                base.replace("\"userName\"", "1"),
                "directory.user_attribute",
            ),
            (
                format!("{base}group_attribute = \"name\"\n"),
                "directory.group_attribute",
            ),
        ] {
            let Err((found, _)) = validated(&src) else {
                panic!("{src}")
            };
            assert_eq!(found, key, "{src}");
        }
    }
}
