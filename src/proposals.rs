//! The proposal server's section (ADR 0022, feature `proposals`): `[proposals]` names the URL
//! agents reach `meada workspaces mcp` at and the audience its tokens carry. The server itself is
//! the console's workspaces module answering over MCP with persistence off
//! (`ui/workspaces/proposals.rs`).

use serde::Deserialize;

use crate::modules::{Problem, Shape};
use crate::msg::Msg;

/// `[proposals]` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    public_url: Option<String>,
    audience: Option<String>,
}

/// The validated section.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The server's own URL, with no trailing slash: the resource its metadata names.
    pub public_url: String,
    /// The `aud` its tokens carry (RFC 8707): `public_url` by default, and never a node's or the
    /// console's, so no token is accepted on two surfaces.
    pub audience: String,
}

impl File {
    /// The section checked: the first problem.
    pub fn validate(self) -> Result<Config, Problem> {
        let Some(public_url) = self.public_url else {
            return Err(("proposals", Msg::KeyMissing("proposals.public_url").text()));
        };
        if !crate::modules::reachable(&public_url, Shape::Bare) {
            return Err(("proposals.public_url", Msg::OidcUrl { bare: true }.text()));
        }
        if self.audience.as_deref() == Some("") {
            return Err(("proposals.audience", Msg::OidcEmpty.text()));
        }
        let public_url = public_url.trim_end_matches('/').to_string();
        Ok(Config {
            audience: self.audience.unwrap_or_else(|| public_url.clone()),
            public_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(toml: &str) -> Result<Config, Problem> {
        toml::from_str::<File>(toml).unwrap().validate()
    }

    #[test]
    fn the_section_needs_a_reachable_url_and_defaults_its_audience_to_it() {
        let config = section("public_url = \"https://proposals.example/\"").unwrap();
        assert_eq!(config.public_url, "https://proposals.example");
        assert_eq!(config.audience, "https://proposals.example");
        let config = section("public_url = \"http://127.0.0.1:9\"\naudience = \"agents\"").unwrap();
        assert_eq!(config.audience, "agents");
        let key = |toml: &str| section(toml).unwrap_err().0;
        assert_eq!(key(""), "proposals");
        assert_eq!(
            key("public_url = \"http://proposals.example\""),
            "proposals.public_url"
        );
        assert_eq!(
            key("public_url = \"https://p.example/path\""),
            "proposals.public_url"
        );
        assert_eq!(
            key("public_url = \"https://p.example\"\naudience = \"\""),
            "proposals.audience"
        );
        assert!(toml::from_str::<File>("public_url = \"https://p\"\nextra = 1").is_err());
    }
}
