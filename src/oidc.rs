//! The OIDC module (ADR 0012, feature `oidc`, section `[oidc]`): server mode as an OAuth 2.1
//! resource server, as the MCP authorization specification (2026-07-28) describes. The proxy
//! stays the default admission (ADR 0011); with `[oidc]`, every request carries
//! `Authorization: Bearer <JWT>` issued by the operator's provider.
//!
//! At start (`Oidc::start`, before the listener binds) the issuer's metadata (RFC 8414 or OpenID
//! discovery, its `issuer` checked) names the `jwks_uri`, and the JWKS is read; a failure
//! refuses the start. Both go through ureq with no redirect, no proxy, `FETCH_TIMEOUT` and a
//! size cap (`METADATA_MAX`, `JWKS_MAX`). A thread refreshes the JWKS every
//! `jwks_refresh_seconds`, and a token naming a `kid` the cached set lacks refreshes it, at most
//! once per `KID_REFRESH_EVERY`. A failed refresh keeps the cached keys and says so on stderr.
//!
//! `Oidc::admit` runs before routing, after `mcp::route` refused a request carrying `Origin` (a
//! browser) with 403: it serves the protected resource metadata (RFC 9728) of any path, without
//! a token, and checks the token: an asymmetric `alg` that fits the key, the signature, `iss`,
//! `aud`, `exp` (required) and `nbf` with `LEEWAY`, and every configured scope. A missing or invalid token is
//! a 401 naming the metadata. What passes is a `Grant`: the identities the token's
//! `identities_claim` lists, and those `[oidc.groups]` maps the groups of its `groups_claim` to
//! (#291; `*` from either for all): the identity provider's claims grant, the one grant provider
//! a grant provider of the union (ADR 0016). `type` names the identity provider; none changes what
//! a token is granted, and `[oidc.entra] overage = "ignore"` leaves a token in overage no groups,
//! while `"directory"` gives it those the directory holds for its user (#336, `overage`).
//! `[oidc.github]` adds the GitHub grant provider (#332, `permissions`): the identities the
//! token's `login_claim` login holds an accepted role on in the newest permissions set `meada
//! github-app roles` wrote (#331), while that set is at most `max_age_seconds` old, never `*`.
//! Routing checks it (403) and lists it (404), and it bounds the workspace of every answer behind
//! it (#290): a repository outside it is absent. An identity the
//! server does not serve grants nothing.
//!
//! Meada never forwards a token, and no error or stderr line holds one or any of its parts (a
//! group, an identity): the 401 and 403 bodies are fixed, and a refused token is never described.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Bytes;
use hyper::{HeaderMap, Response, StatusCode, header};
use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, KeyOperations, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Header, Validation};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Map, Value};
use ureq::http::Uri;

use crate::modules::{Shape, agent, reachable};
use crate::msg::Msg;

/// An answer of the server.
pub type Reply = Response<BoxBody<Bytes, Infallible>>;

// ponytail: how long one metadata or JWKS request may take, connection to last byte.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
// ponytail: the most bytes of an issuer's metadata document read; past it the document fails.
const METADATA_MAX: u64 = 64 << 10;
// ponytail: the most bytes of a JWKS read; past it the fetch fails.
const JWKS_MAX: u64 = 256 << 10;
/// The least time between two JWKS fetches an unknown `kid` causes.
const KID_REFRESH_EVERY: Duration = Duration::from_secs(60);
/// The seconds of clock skew `exp` and `nbf` tolerate.
const LEEWAY: u64 = 60;
/// The shortest `jwks_refresh_seconds`, and its default.
const REFRESH_MIN: i64 = 60;
const REFRESH_DEFAULT: i64 = 3600;
/// The algorithms a token may be signed with: asymmetric ones only, so no `none` and no HMAC.
const ALGORITHMS: [Algorithm; 6] = [
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::PS256,
];
/// Where a resource's metadata lives, inserted before its path (RFC 9728).
const METADATA_PATH: &str = ".well-known/oauth-protected-resource";
/// What a path keeps encoded in a URL: everything but the characters a path may hold as is.
const PATH: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// The identity provider's `type`s (ADR 0016): `okta` and `keycloak` behave as `generic`, and
/// only `entra` takes a table of its own.
const TYPES: [&str; 4] = ["generic", "entra", "okta", "keycloak"];

/// What `[oidc.entra] overage` accepts: `ignore`, a token in overage having no groups, and
/// `directory`, its groups from the directory the identity provider pushes (#336).
const OVERAGES: [&str; 2] = ["ignore", "directory"];

/// `[oidc.github] roles` by default: GitHub's five built-in repository roles, as `role_name`
/// spells them.
const ROLES: [&str; 5] = ["read", "triage", "write", "maintain", "admin"];
// ponytail: the longest role name `[oidc.github] roles` takes, in characters, and the longest
// `role_name` from GitHub the role check stores; a longer one is refused (a skipped row).
const ROLE_MAX: usize = 100;
/// The shortest `[oidc.github] max_age_seconds`.
const MAX_AGE_MIN: i64 = 60;

/// The shortest `[oidc.github] max_age_seconds` this process takes: `MAX_AGE_MIN`, or 1 in a debug
/// build with `MEADA_PERMISSIONS_POLL_MS` set, so a test crosses the age in seconds.
fn max_age_min() -> i64 {
    #[cfg(all(debug_assertions, feature = "postgres"))]
    if permissions::poll_knob().is_some() {
        return 1;
    }
    MAX_AGE_MIN
}

#[cfg(feature = "postgres")]
mod permissions;

#[cfg(feature = "directory")]
mod overage;

/// `[oidc.entra] overage = "directory"` (#336): where the directory is read. `Manifest::read`
/// sets it from `[directory]` and `[postgres]`, and refuses the key without them.
#[derive(Clone, Debug, PartialEq)]
pub struct Overage {
    #[cfg(feature = "directory")]
    pub lookup: Option<(crate::directory::Config, crate::postgres::Config)>,
}

/// The `[oidc]` section as written: the identity provider.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    /// Any value, so that one of another type is the error naming the accepted ones.
    #[serde(rename = "type")]
    kind: Option<toml::Value>,
    issuer: String,
    public_url: String,
    audience: Option<String>,
    identities_claim: Option<String>,
    groups_claim: Option<String>,
    /// `[oidc.groups]`: a group to the identities it grants.
    groups: Option<HashMap<String, Vec<String>>>,
    scopes: Option<Vec<String>>,
    jwks_refresh_seconds: Option<i64>,
    entra: Option<Entra>,
    github: Option<GithubFile>,
}

/// `[oidc.entra]`: the settings only Entra ID needs.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entra {
    overage: Option<toml::Value>,
}

/// `[oidc.github]`: the GitHub grant provider (ADR 0016).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubFile {
    host: Option<String>,
    login_claim: Option<String>,
    roles: Option<Vec<String>>,
    max_age_seconds: Option<i64>,
}

/// The validated `[oidc.github]`: what `meada github-app roles` checks against (#331) and the
/// grant at admission reads (#332).
#[derive(Clone, Debug, PartialEq)]
pub struct Github {
    /// The git host whose identities it grants, a lowercase host name.
    pub host: String,
    /// The top-level claim holding a subject's GitHub login.
    pub login_claim: String,
    /// The accepted roles, compared exactly with GitHub's `role_name`.
    pub roles: Vec<String>,
    /// How old the newest permissions set may be and still grant.
    pub max_age: Duration,
    /// The manifest's `[postgres]`, where the set is read: `Manifest::read` sets it, and
    /// refuses `[oidc.github]` without it.
    #[cfg(feature = "postgres")]
    pub postgres: Option<crate::postgres::Config>,
}

/// Whether `role` is a role name `[oidc.github] roles` takes: 1 to `ROLE_MAX` characters with no
/// control character. GitHub's `role_name`s are checked alike before they are stored.
pub(crate) fn role_ok(role: &str) -> bool {
    (1..=ROLE_MAX).contains(&role.chars().count()) && !role.chars().any(char::is_control)
}

impl GithubFile {
    fn validate(self) -> Result<Github, crate::modules::Problem> {
        // Without the data layer there is no permissions set to read or write.
        if !cfg!(feature = "postgres") {
            let (section, feature) = ("oidc.github", "postgres");
            return Err(("oidc.github", Msg::NeedsFeature { section, feature }.text()));
        }
        let host = self.host.unwrap_or_else(|| "github.com".into());
        let named = !host.is_empty()
            && (host.bytes()).all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.'));
        if !named {
            return Err(("oidc.github.host", Msg::OidcGithubHost.text()));
        }
        let login_claim = match self.login_claim {
            None => {
                let missing = Msg::KeyMissing("oidc.github.login_claim").text();
                return Err(("oidc.github", missing));
            }
            Some(claim) if claim.is_empty() => {
                return Err(("oidc.github.login_claim", Msg::OidcEmpty.text()));
            }
            Some(claim) => claim,
        };
        let roles = (self.roles).unwrap_or_else(|| ROLES.map(str::to_string).to_vec());
        let distinct = (roles.iter().enumerate()).all(|(n, r)| !roles[..n].contains(r));
        if roles.is_empty() || !roles.iter().all(|r| role_ok(r)) || !distinct {
            return Err(("oidc.github.roles", Msg::OidcGithubRoles(ROLE_MAX).text()));
        }
        let key = "oidc.github.max_age_seconds";
        let Some(max_age) = self.max_age_seconds else {
            return Err(("oidc.github", Msg::KeyMissing(key).text()));
        };
        let floor = max_age_min();
        let max_age = crate::modules::at_least(key, Some(max_age), floor, floor)?;
        Ok(Github {
            host,
            login_claim,
            roles,
            max_age: Duration::from_secs(max_age),
            #[cfg(feature = "postgres")]
            postgres: None,
        })
    }
}

/// The validated section.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The identity provider's `type`, one of `TYPES`.
    pub kind: &'static str,
    /// The issuer, as tokens and its metadata name it.
    pub issuer: String,
    /// The URL clients use, with no trailing slash: `<public_url>/<identity>` is a resource.
    pub public_url: String,
    /// The `aud` every token carries; without it, the resource the path names.
    pub audience: Option<String>,
    /// The claim listing the identities a token may read.
    pub claim: Option<String>,
    /// The claim listing a token's groups, and the identities each group grants.
    pub groups: Option<(String, HashMap<String, Vec<String>>)>,
    /// The scopes announced and required.
    pub scopes: Vec<String>,
    /// How often the JWKS is fetched again.
    pub refresh: Duration,
    /// `[oidc.github]`, the GitHub grant provider.
    pub github: Option<Github>,
    /// `[oidc.entra] overage = "directory"`, a token in overage getting its groups from the
    /// directory.
    pub overage: Option<Overage>,
}

impl File {
    pub fn validate(self) -> Result<Config, crate::modules::Problem> {
        if !reachable(&self.issuer, Shape::Path) {
            return Err(("oidc.issuer", Msg::OidcUrl { bare: false }.text()));
        }
        if !reachable(&self.public_url, Shape::Bare) {
            return Err(("oidc.public_url", Msg::OidcUrl { bare: true }.text()));
        }
        if self.audience.as_deref() == Some("") {
            return Err(("oidc.audience", Msg::OidcEmpty.text()));
        }
        if self.identities_claim.as_deref() == Some("") {
            return Err(("oidc.identities_claim", Msg::OidcEmpty.text()));
        }
        let groups = match (self.groups_claim, self.groups) {
            (Some(claim), _) if claim.is_empty() => {
                return Err(("oidc.groups_claim", Msg::OidcEmpty.text()));
            }
            (Some(claim), Some(table)) => Some((claim, table)),
            (Some(_), None) => {
                return Err(("oidc.groups_claim", Msg::KeyMissing("oidc.groups").text()));
            }
            (None, Some(_)) => {
                return Err(("oidc.groups", Msg::KeyMissing("oidc.groups_claim").text()));
            }
            (None, None) => None,
        };
        let github = self.github.map(GithubFile::validate).transpose()?;
        // The GitHub grant provider alone may grant: no claim is needed beside it.
        if self.identities_claim.is_none() && groups.is_none() && github.is_none() {
            return Err(("oidc.identities_claim", Msg::OidcNoClaim.text()));
        }
        let scopes = self.scopes.unwrap_or_default();
        for scope in &scopes {
            // RFC 6749's scope-token: what a quoted `scope` parameter can carry as is.
            let bad = (scope.chars()).find(|c| !matches!(c, '!' | '#'..='[' | ']'..='~'));
            match bad {
                _ if scope.is_empty() => return Err(("oidc.scopes", Msg::OidcEmpty.text())),
                Some(bad) => return Err(("oidc.scopes", Msg::OidcScope { scope, bad }.text())),
                None => {}
            }
        }
        let key = "oidc.jwks_refresh_seconds";
        let seconds = self.jwks_refresh_seconds;
        let refresh = crate::modules::at_least(key, seconds, REFRESH_DEFAULT, REFRESH_MIN)?;
        let refresh = Duration::from_secs(refresh);
        let one_of = |value: &toml::Value, accepted: &[&'static str]| {
            (accepted.iter().copied()).find(|&a| value.as_str() == Some(a))
        };
        let kind = match &self.kind {
            None => "generic",
            Some(value) => {
                one_of(value, &TYPES).ok_or(("oidc.type", Msg::OidcOneOf(&TYPES).text()))?
            }
        };
        let mut overage = None;
        if let Some(entra) = self.entra {
            if kind != "entra" {
                return Err(("oidc.entra", Msg::OidcTypeTable("entra").text()));
            }
            if let Some(value) = &entra.overage {
                let key = "oidc.entra.overage";
                let chosen =
                    one_of(value, &OVERAGES).ok_or((key, Msg::OidcOneOf(&OVERAGES).text()))?;
                if chosen == "directory" {
                    // The directory is in the data layer, which only `directory` builds have.
                    if !cfg!(feature = "directory") {
                        let (section, feature) = ("oidc.entra", "directory");
                        return Err((key, Msg::NeedsFeature { section, feature }.text()));
                    }
                    if groups.is_none() {
                        return Err((key, Msg::KeyMissing("oidc.groups_claim").text()));
                    }
                    overage = Some(Overage {
                        #[cfg(feature = "directory")]
                        lookup: None,
                    });
                }
            }
        }
        Ok(Config {
            kind,
            issuer: self.issuer,
            public_url: self.public_url.trim_end_matches('/').to_string(),
            audience: self.audience,
            claim: self.identities_claim,
            groups,
            scopes,
            refresh,
            github,
            overage,
        })
    }
}

/// What a token admitted may read.
#[derive(Clone)]
pub enum Grant {
    /// Its claim lists `*`, or one of its groups maps to it.
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

    /// The identity provider's claims grant of `claims` under `config`, its `groups` being those
    /// the grant reads (the claim's, or the directory's for a token in overage), `*` for all.
    fn of(claims: &Map<String, Value>, config: &Config, groups: &[String]) -> Self {
        let listed: HashSet<String> = (claims_grant(claims, config, groups))
            .map(str::to_string)
            .collect();
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

/// What a token's grant is made of (ADR 0016): the claims grant and, with `[oidc.github]`, the
/// token's login and the GitHub grant (empty without a valid login or a current set).
struct Granted {
    claims: Grant,
    /// The groups the claims grant read: the token's `groups_claim`, or the directory's for a
    /// token in overage (#336).
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    groups: Vec<String>,
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    login: Option<String>,
    github: Option<Vec<String>>,
}

impl Granted {
    /// The grant every check reads: the union of both providers.
    fn union(&self) -> Grant {
        self.claims.with(self.github.as_deref().unwrap_or_default())
    }
}

/// The identity provider's claims grant: the identities `claims`' `identities_claim` lists and
/// those `[oidc.groups]` maps `groups` to. A claim that is not an array of strings, and a group
/// the table lacks, grant nothing.
fn claims_grant<'a>(
    claims: &'a Map<String, Value>,
    config: &'a Config,
    groups: &'a [String],
) -> impl Iterator<Item = &'a str> + use<'a> {
    let listed = config.claim.iter().flat_map(|claim| strings(claims, claim));
    let mapped = (config.groups.iter()).flat_map(|(_, table)| {
        (groups.iter().filter_map(|group| table.get(group)))
            .flatten()
            .map(String::as_str)
    });
    listed.chain(mapped)
}

/// The strings of `claims`' array `claim`; none when it is no array or holds anything but
/// strings, so a malformed claim fails closed.
fn strings<'a>(
    claims: &'a Map<String, Value>,
    claim: &str,
) -> impl Iterator<Item = &'a str> + use<'a> {
    let array = claims.get(claim).and_then(Value::as_array);
    let strings = array.and_then(|a| a.iter().map(Value::as_str).collect::<Option<Vec<_>>>());
    strings.into_iter().flatten()
}

/// Why a token is not admitted.
#[derive(Debug, PartialEq)]
enum Refusal {
    /// No `Authorization` header.
    Missing,
    /// A header or token that does not pass.
    Invalid,
    /// A valid token without every configured scope.
    Scope,
}

/// The module of a running server: its section, the issuer's metadata and `jwks_uri`, and the
/// cached keys.
pub struct Oidc {
    config: Config,
    /// The issuer's metadata document, as read at start.
    #[cfg_attr(not(feature = "ui"), allow(dead_code))]
    issuer_metadata: Value,
    jwks_uri: String,
    agent: ureq::Agent,
    keys: RwLock<Arc<Vec<Jwk>>>,
    /// When an unknown `kid` last made the module fetch the JWKS.
    kid_fetched: Mutex<Option<Instant>>,
    /// When the JWKS was last read, and the refresh that failed since.
    fetched: Mutex<Fetched>,
    /// The permissions cache of `[oidc.github]` (#332).
    #[cfg(feature = "postgres")]
    permissions: Option<Arc<permissions::Permissions>>,
    /// The overage lookup and its cache, with `[oidc.entra] overage = "directory"` (#336).
    #[cfg(feature = "directory")]
    overage: Option<overage::Overage>,
}

/// When the cached JWKS was read (seconds since the epoch), and the last refresh that failed
/// after it, with its time and why.
#[cfg_attr(not(feature = "ui"), allow(dead_code))]
struct Fetched {
    at: u64,
    failed: Option<(u64, String)>,
}

/// A console's admission (ADR 0014): what a token admitted may read, and who it names.
#[cfg(feature = "ui")]
pub struct Admitted {
    /// The token's `sub`, when it has one.
    pub subject: Option<String>,
    pub grant: Grant,
    /// The token's `groups_claim` read as the grant reads it, sorted, without repeats.
    pub groups: Vec<String>,
    /// The console role derived only from verified groups or a strict boolean claim.
    pub operator: bool,
    /// The token's `[directory] user_claim`, when it is a string: what the directory is asked
    /// about (#335). Never printed.
    #[cfg_attr(not(feature = "directory"), allow(dead_code))]
    pub user: Option<String>,
    /// With `[oidc.github]`, the token's login when `login_claim` holds a valid one (#332).
    pub login: Option<String>,
}

/// Each check of a pasted token (ADR 0014): `None` where it could not be made, since the
/// signature did not pass.
#[cfg(feature = "ui")]
pub struct Checked {
    pub signature: bool,
    pub issuer: Option<bool>,
    pub audience: Option<bool>,
    pub lifetime: Option<bool>,
    pub scopes: Option<bool>,
    /// What the token may read when every check passes.
    pub grant: Option<Grant>,
    /// With `[oidc.github]` and every check passed, the grant's two sides: the claims grant
    /// and the GitHub grant, from the cache (#332).
    pub sources: Option<(Grant, Vec<String>)>,
}

/// `GET url`, at most `max` bytes of a success answer; a redirect is no success.
fn get(agent: &ureq::Agent, url: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut response =
        (agent.get(url).header("Accept", "application/json").call()).map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("http status {}", status.as_u16()));
    }
    // ureq refuses a body that fills its limit, so `max` bytes take a limit one past it.
    let body = response.body_mut().with_config().limit(max + 1);
    body.read_to_vec().map_err(|e| e.to_string())
}

/// The metadata URLs of `issuer`, in the order the MCP authorization specification tries them:
/// RFC 8414 and OpenID discovery with the path inserted, then OpenID's with it appended.
fn metadata_urls(issuer: &str) -> Vec<String> {
    let Ok(uri) = issuer.parse::<Uri>() else {
        return Vec::new();
    };
    let origin = format!(
        "{}://{}",
        uri.scheme_str().unwrap_or_default(),
        uri.authority().map_or("", |a| a.as_str())
    );
    let path = uri.path().trim_end_matches('/');
    let mut urls = vec![
        format!("{origin}/.well-known/oauth-authorization-server{path}"),
        format!("{origin}/.well-known/openid-configuration{path}"),
    ];
    if !path.is_empty() {
        urls.push(format!("{origin}{path}/.well-known/openid-configuration"));
    }
    urls
}

/// The `jwks_uri` of the first metadata document of `issuer` that can be used, and the document:
/// it must name `issuer` (RFC 8414) and an endpoint the module may reach. A document that cannot
/// is passed over as an unanswered one is, and each is named in the error when none can.
fn discover(agent: &ureq::Agent, issuer: &str) -> Result<(String, Value), String> {
    let mut tried = Vec::new();
    for url in metadata_urls(issuer) {
        match metadata(agent, &url, issuer) {
            Ok(found) => return Ok(found),
            Err(why) => tried.push(format!("{url}: {why}")),
        }
    }
    Err(Msg::OidcMetadata(&tried.join("; ")).text())
}

/// The `jwks_uri` of the metadata document at `url`, and the document, or why it cannot be used.
fn metadata(agent: &ureq::Agent, url: &str, issuer: &str) -> Result<(String, Value), String> {
    #[derive(Deserialize)]
    struct Metadata {
        issuer: String,
        jwks_uri: Option<String>,
    }
    let body = get(agent, url, METADATA_MAX)?;
    let document: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let metadata = Metadata::deserialize(&document).map_err(|e| e.to_string())?;
    if metadata.issuer != issuer {
        return Err(Msg::OidcIssuerMismatch(&metadata.issuer).text());
    }
    let jwks_uri = metadata.jwks_uri.unwrap_or_default();
    match reachable(&jwks_uri, Shape::Query) {
        true => Ok((jwks_uri, document)),
        false => Err(Msg::OidcJwksUri(&Msg::OidcJwksUrl.text()).text()),
    }
}

/// The signing keys of the JWKS at `url` the module accepts: RSA, or EC on P-256 or P-384, for
/// signatures; any other key is left out, and a set with none fails.
fn jwks(agent: &ureq::Agent, url: &str) -> Result<Vec<Jwk>, String> {
    #[derive(Deserialize)]
    struct Set {
        keys: Vec<Value>,
    }
    let fail = |why: String| Msg::OidcJwks { url, why: &why }.text();
    let body = get(agent, url, JWKS_MAX).map_err(fail)?;
    // Only where the document fails, never what it holds: a failed refresh is shown (ADR 0014).
    let shape = |e: serde_json::Error| {
        let (line, column) = (e.line(), e.column());
        fail(Msg::OidcJwksShape { line, column }.text())
    };
    let set: Set = serde_json::from_slice(&body).map_err(shape)?;
    let keys: Vec<Jwk> = (set.keys.into_iter())
        .filter_map(|key| serde_json::from_value::<Jwk>(key).ok())
        .filter(signs)
        .collect();
    match keys.is_empty() {
        true => Err(fail(Msg::OidcJwksNoKey.text())),
        false => Ok(keys),
    }
}

/// Whether `key` may verify a token's signature.
fn signs(key: &Jwk) -> bool {
    let common = &key.common;
    let usable = matches!(common.public_key_use, None | Some(PublicKeyUse::Signature))
        && (common.key_operations.as_ref()).is_none_or(|ops| ops.contains(&KeyOperations::Verify));
    let named = (common.key_algorithm.as_ref()).is_none_or(|a| algorithm(a).is_some());
    let kind = match &key.algorithm {
        AlgorithmParameters::RSA(_) => true,
        AlgorithmParameters::EllipticCurve(p) => {
            matches!(p.curve, EllipticCurve::P256 | EllipticCurve::P384)
        }
        _ => false,
    };
    usable && named && kind
}

/// The algorithm a key names, when it is one a token may use.
fn algorithm(named: &jsonwebtoken::jwk::KeyAlgorithm) -> Option<Algorithm> {
    let algorithm = Algorithm::from_str(&named.to_string()).ok()?;
    ALGORITHMS.contains(&algorithm).then_some(algorithm)
}

/// Whether `key` is the one `header` names and fits its algorithm: the same `kid` when both
/// have one, the algorithm the key names when it names one, and its type and curve.
fn fits(key: &Jwk, header: &Header) -> bool {
    let kid = match (header.kid.as_deref(), key.common.key_id.as_deref()) {
        (Some(named), Some(kid)) => named == kid,
        _ => true,
    };
    let named =
        (key.common.key_algorithm.as_ref()).is_none_or(|a| algorithm(a) == Some(header.alg));
    let kind = match (&key.algorithm, header.alg) {
        (AlgorithmParameters::RSA(_), alg) => matches!(
            alg,
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 | Algorithm::PS256
        ),
        (AlgorithmParameters::EllipticCurve(p), Algorithm::ES256) => p.curve == EllipticCurve::P256,
        (AlgorithmParameters::EllipticCurve(p), Algorithm::ES384) => p.curve == EllipticCurve::P384,
        _ => false,
    };
    kid && named && kind
}

/// The token of `headers`' one `Authorization: Bearer` header: `Ok(None)` without one, or with
/// another scheme (RFC 6750 §3.1: no bearer token was sent); two headers, a value that is not
/// UTF-8 and an empty token are refused.
fn bearer(headers: &HeaderMap) -> Result<Option<&str>, Refusal> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(Refusal::Invalid);
    }
    let value = value.to_str().map_err(|_| Refusal::Invalid)?;
    let (scheme, token) = value.split_once(' ').unwrap_or((value, ""));
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Ok(None);
    }
    match token.trim() {
        "" => Err(Refusal::Invalid),
        token => Ok(Some(token)),
    }
}

/// The header of `token` when it may be checked: an algorithm of `ALGORITHMS`, and no `crit`,
/// which names extensions the token needs understood (RFC 7515 §4.1.11): none is.
fn signing(token: &str) -> Option<Header> {
    let header = jsonwebtoken::decode_header(token).ok()?;
    (ALGORITHMS.contains(&header.alg) && header.crit.is_none()).then_some(header)
}

/// Whether `claims`' `aud`, a string or an array, holds one of `audiences`.
fn addressed(claims: &Map<String, Value>, audiences: &[String]) -> bool {
    let aud = match claims.get("aud") {
        Some(Value::String(aud)) => vec![aud.as_str()],
        Some(Value::Array(auds)) => auds.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    aud.iter().any(|aud| audiences.iter().any(|a| a == aud))
}

/// A response with a fixed body.
fn reply(status: StatusCode, kind: &str, body: String) -> Reply {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, kind)
        .body(Full::from(body).boxed())
        .expect("a valid response")
}

impl Oidc {
    /// The module of `config`: the issuer's metadata and its JWKS read, then a thread that
    /// fetches the JWKS again every `config.refresh` while the module lives. With
    /// `[oidc.github]`, the permissions cache over its `[postgres]`, empty until `listening`.
    pub fn start(config: Config) -> crate::Result<Arc<Self>> {
        let agent = agent(FETCH_TIMEOUT);
        let (jwks_uri, issuer_metadata) = discover(&agent, &config.issuer)?;
        let keys = jwks(&agent, &jwks_uri)?;
        #[cfg(feature = "postgres")]
        let permissions = (config.github.clone())
            .and_then(|github| Some((github.postgres.clone()?, github)))
            .map(|(postgres, github)| permissions::Permissions::new(github, postgres));
        #[cfg(feature = "directory")]
        let overage = (config.overage.as_ref())
            .and_then(|overage| overage.lookup.as_ref())
            .map(|(directory, postgres)| overage::Overage::new(directory, postgres.clone()));
        let oidc = Self::new(config, agent, jwks_uri, keys);
        let oidc = Arc::new(Self {
            issuer_metadata,
            #[cfg(feature = "postgres")]
            permissions,
            #[cfg(feature = "directory")]
            overage,
            ..oidc
        });
        let (module, every) = (Arc::downgrade(&oidc), oidc.config.refresh);
        std::thread::spawn(move || refresh_every(module, every));
        Ok(oidc)
    }

    fn new(config: Config, agent: ureq::Agent, jwks_uri: String, keys: Vec<Jwk>) -> Self {
        Self {
            config,
            jwks_uri,
            agent,
            issuer_metadata: Value::Null,
            keys: RwLock::new(Arc::new(keys)),
            kid_fetched: Mutex::new(None),
            fetched: Mutex::new(Fetched {
                at: crate::modules::now(),
                failed: None,
            }),
            #[cfg(feature = "postgres")]
            permissions: None,
            #[cfg(feature = "directory")]
            overage: None,
        }
    }

    /// Starts reading the permissions set (`[oidc.github]`, #332), which admission never waits
    /// on: a listener calls it once, after its address is the first line on stderr.
    pub fn listening(&self) {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.permissions {
            cache.poll();
        }
    }

    /// The grant of `claims`, its sides and its login: with `[oidc.github]`, the GitHub grant of
    /// a valid `login_claim` login, read from the cache (never from Postgres).
    fn granted(&self, claims: &Map<String, Value>) -> Granted {
        let groups = self.groups(claims);
        let claims_grant = Grant::of(claims, &self.config, &groups);
        #[cfg(feature = "postgres")]
        if let (Some(github), Some(cache)) = (&self.config.github, &self.permissions) {
            let login = permissions::login(claims.get(&github.login_claim));
            let read = login.as_deref().map(|l| cache.grant(l)).unwrap_or_default();
            return Granted {
                claims: claims_grant,
                groups,
                login,
                github: Some(read),
            };
        }
        Granted {
            claims: claims_grant,
            groups,
            login: None,
            github: None,
        }
    }

    /// The groups of `claims` as every check reads them: its `groups_claim` array (none when it
    /// is no array of strings), or, for a token in overage under `overage = "directory"`, those
    /// the directory holds for its user (#336).
    fn groups(&self, claims: &Map<String, Value>) -> Vec<String> {
        let Some((claim, _)) = &self.config.groups else {
            return Vec::new();
        };
        #[cfg(feature = "directory")]
        if let Some(overage) = &self.overage
            && overage::in_overage(claims, claim)
        {
            return overage.groups(claims);
        }
        strings(claims, claim).map(str::to_string).collect()
    }

    /// Whether `claims` is a token in overage whose groups the directory gives (#336).
    #[cfg(feature = "ui")]
    fn overage(&self, claims: &Map<String, Value>) -> bool {
        #[cfg(feature = "directory")]
        if let (Some(_), Some((claim, _))) = (&self.overage, &self.config.groups) {
            return overage::in_overage(claims, claim);
        }
        let _ = claims;
        false
    }

    /// With `[oidc.github]`, its host and the GitHub grant's state (`current`, `expired` or
    /// `unread`), whatever a token's login (#332).
    #[cfg(feature = "ui")]
    pub fn github(&self) -> Option<(&str, &'static str)> {
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.permissions {
            return Some((cache.host(), cache.state()));
        }
        None
    }

    /// Whether the request to `path` (percent-decoded, without its leading and trailing
    /// slashes) goes on to routing, and with what grant; `identity` is the repository `path`
    /// names (`path` without its pin, if it has one). Otherwise the answer: the protected
    /// resource metadata of a path under `.well-known/oauth-protected-resource`, with no token
    /// needed; 401 for a missing or invalid token and 403 for a missing scope. Blocks on the
    /// JWKS fetch an unknown `kid` causes. A request carrying `Origin` never reaches it
    /// (`mcp::route` refuses it first).
    pub fn admit(
        &self,
        headers: &HeaderMap,
        path: &str,
        identity: &str,
    ) -> Result<Grant, Box<Reply>> {
        if let Some(resource) = path.strip_prefix(METADATA_PATH)
            && (resource.is_empty() || resource.starts_with('/'))
        {
            return Err(Box::new(self.metadata(resource.trim_start_matches('/'))));
        }
        let audiences = match &self.config.audience {
            Some(audience) => vec![audience.clone()],
            None => vec![self.resource(path), self.resource(identity)],
        };
        let refused = match bearer(headers) {
            Ok(Some(token)) => match self.verify(token, &audiences) {
                Ok(grant) => return Ok(grant),
                Err(refusal) => refusal,
            },
            Ok(None) => Refusal::Missing,
            Err(refusal) => refusal,
        };
        Err(Box::new(self.refusal(path, &refused)))
    }

    /// The 403 for a valid token whose claim does not list the identity `path` names.
    pub fn forbidden(&self, path: &str) -> Reply {
        let challenge = self.challenge(Some(path), Some("insufficient_scope"), false);
        self.challenged(StatusCode::FORBIDDEN, challenge)
    }

    fn refusal(&self, path: &str, refusal: &Refusal) -> Reply {
        let (status, error) = match refusal {
            Refusal::Missing => (StatusCode::UNAUTHORIZED, None),
            Refusal::Invalid => (StatusCode::UNAUTHORIZED, Some("invalid_token")),
            Refusal::Scope => (StatusCode::FORBIDDEN, Some("insufficient_scope")),
        };
        self.challenged(status, self.challenge(Some(path), error, true))
    }

    fn challenged(&self, status: StatusCode, challenge: String) -> Reply {
        let body = match status {
            StatusCode::UNAUTHORIZED => "unauthorized\n",
            _ => "forbidden\n",
        };
        let mut response = reply(status, "text/plain; charset=utf-8", body.into());
        if let Ok(value) = header::HeaderValue::from_str(&challenge) {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }
        response
    }

    /// `WWW-Authenticate` for `path`: the error, the metadata URL (none without a path: the
    /// console serves no metadata) and, with `scope`, the configured scopes.
    fn challenge(&self, path: Option<&str>, error: Option<&str>, scope: bool) -> String {
        let mut parts = Vec::new();
        if let Some(error) = error {
            parts.push(format!("error=\"{error}\""));
        }
        if let Some(path) = path {
            let metadata = self.resource(&format!("{METADATA_PATH}/{path}"));
            parts.push(format!(
                "resource_metadata=\"{}\"",
                metadata.trim_end_matches('/')
            ));
        }
        if scope && !self.config.scopes.is_empty() {
            parts.push(format!("scope=\"{}\"", self.config.scopes.join(" ")));
        }
        match parts.is_empty() {
            true => "Bearer".into(),
            false => format!("Bearer {}", parts.join(", ")),
        }
    }

    /// The URL of the resource at `path`: `<public_url>/<path>`, the path percent-encoded.
    fn resource(&self, path: &str) -> String {
        let public = &self.config.public_url;
        match path.is_empty() {
            true => public.clone(),
            false => format!("{public}/{}", utf8_percent_encode(path, PATH)),
        }
    }

    /// The protected resource metadata of the resource at `path` (RFC 9728).
    fn metadata(&self, path: &str) -> Reply {
        let mut document = serde_json::json!({
            "resource": self.resource(path),
            "authorization_servers": [self.config.issuer],
            "bearer_methods_supported": ["header"],
        });
        if !self.config.scopes.is_empty() {
            document["scopes_supported"] = self.config.scopes.clone().into();
        }
        reply(StatusCode::OK, "application/json", document.to_string())
    }

    /// The grant of `token` if it passes (`verified`): the union of its providers' grants.
    fn verify(&self, token: &str, audiences: &[String]) -> Result<Grant, Refusal> {
        let claims = self.verified(token, audiences)?;
        Ok(self.granted(&claims).union())
    }

    /// The claims of `token` if it passes: an algorithm of `ALGORITHMS`, signed by a cached key
    /// that fits it (the JWKS fetched again once when none does), `exp` and `nbf`, `iss`, an
    /// `aud` among `audiences`, and every configured scope.
    fn verified(&self, token: &str, audiences: &[String]) -> Result<Map<String, Value>, Refusal> {
        let header = signing(token).ok_or(Refusal::Invalid)?;
        if !self.keys().iter().any(|key| fits(key, &header)) {
            self.refetch();
        }
        let claims = self.signed(token, &header, true).ok_or(Refusal::Invalid)?;
        if !self.issued(&claims) || !addressed(&claims, audiences) {
            return Err(Refusal::Invalid);
        }
        if !self.scoped(&claims) {
            return Err(Refusal::Scope);
        }
        Ok(claims)
    }

    /// The claims of `token`, whose header is `header`, if a cached key that fits it signed it;
    /// with `lifetime`, also `exp` (required) and `nbf` with `LEEWAY`.
    fn signed(&self, token: &str, header: &Header, lifetime: bool) -> Option<Map<String, Value>> {
        let mut validation = Validation::new(header.alg);
        validation.leeway = LEEWAY;
        validation.validate_nbf = lifetime;
        validation.validate_exp = lifetime;
        if !lifetime {
            validation.required_spec_claims.clear();
        }
        // Checked by `verified`, against the claims as signed.
        validation.validate_aud = false;
        (self.keys().iter())
            .filter(|key| fits(key, header))
            .filter_map(|key| DecodingKey::from_jwk(key).ok())
            .find_map(|key| {
                jsonwebtoken::decode::<Map<String, Value>>(token, &key, &validation).ok()
            })
            .map(|data| data.claims)
    }

    fn issued(&self, claims: &Map<String, Value>) -> bool {
        claims.get("iss").and_then(Value::as_str) == Some(&self.config.issuer)
    }

    /// Whether `claims`' `scope` holds every configured scope.
    fn scoped(&self, claims: &Map<String, Value>) -> bool {
        let granted: HashSet<&str> = (claims.get("scope").and_then(Value::as_str))
            .map(|s| s.split(' ').collect())
            .unwrap_or_default();
        (self.config.scopes.iter()).all(|s| granted.contains(s.as_str()))
    }

    /// The console's admission (ADR 0014): `headers`' bearer token checked as `verified` checks
    /// it, its audience `audience`. Otherwise the status and `WWW-Authenticate` refusing it: 401
    /// for a missing or invalid token (`error="invalid_token"` when one was sent), 403 for a
    /// missing scope; no `resource_metadata`, since the console serves none. `user_claim` names
    /// the claim `Admitted::user` reads.
    #[cfg(feature = "ui")]
    pub fn admit_console(
        &self,
        headers: &HeaderMap,
        audience: &str,
        operator_groups: Option<&[String]>,
        operator_claim: Option<&str>,
        user_claim: Option<&str>,
    ) -> Result<Admitted, (StatusCode, String)> {
        let refused = match bearer(headers) {
            Ok(Some(token)) => match self.verified(token, &[audience.to_string()]) {
                Ok(claims) => {
                    let subject = claims.get("sub").and_then(Value::as_str);
                    // The groups the directory gives a token in overage replace its missing
                    // claim here as everywhere (#336).
                    let granted = self.granted(&claims);
                    let in_operators = |group: &str| {
                        operator_groups.is_some_and(|ops| ops.iter().any(|op| op == group))
                    };
                    let by_groups = match self.overage(&claims) {
                        true => granted.groups.iter().any(|group| in_operators(group)),
                        // A token with its own claim keeps the lenient reading it always had: a
                        // string among other elements still matches an operator group.
                        false => self.config.groups.as_ref().is_some_and(|(claim, _)| {
                            let groups = claims.get(claim).and_then(Value::as_array);
                            groups.is_some_and(|groups| {
                                groups.iter().filter_map(Value::as_str).any(in_operators)
                            })
                        }),
                    };
                    let operator = operator_claim
                        .is_some_and(|name| claims.get(name) == Some(&Value::Bool(true)))
                        || by_groups;
                    let mut groups = granted.groups.clone();
                    groups.sort();
                    groups.dedup();
                    return Ok(Admitted {
                        subject: subject.map(str::to_string),
                        grant: granted.union(),
                        groups,
                        operator,
                        user: user_claim
                            .and_then(|name| claims.get(name))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        login: granted.login,
                    });
                }
                Err(refusal) => refusal,
            },
            Ok(None) => Refusal::Missing,
            Err(refusal) => refusal,
        };
        let (status, error) = match refused {
            Refusal::Missing => (StatusCode::UNAUTHORIZED, None),
            Refusal::Invalid => (StatusCode::UNAUTHORIZED, Some("invalid_token")),
            Refusal::Scope => (StatusCode::FORBIDDEN, Some("insufficient_scope")),
        };
        Err((status, self.challenge(None, error, true)))
    }

    /// Each check of a pasted `token` as a node makes it (ADR 0014), against the cached keys
    /// only: an unknown `kid` fetches nothing, so the check sends no request anywhere. Its
    /// audience is a node's: `[oidc] audience`, else the resource of the root or of an identity
    /// of `served`.
    #[cfg(feature = "ui")]
    pub fn check(&self, token: &str, served: &[&str]) -> Checked {
        let failed = Checked {
            signature: false,
            issuer: None,
            audience: None,
            lifetime: None,
            scopes: None,
            grant: None,
            sources: None,
        };
        let Some(header) = signing(token) else {
            return failed;
        };
        let Some(claims) = self.signed(token, &header, false) else {
            return failed;
        };
        let audiences: Vec<String> = match &self.config.audience {
            Some(audience) => vec![audience.clone()],
            None => (std::iter::once(""))
                .chain(served.iter().copied())
                .map(|path| self.resource(path))
                .collect(),
        };
        let issuer = self.issued(&claims);
        let audience = addressed(&claims, &audiences);
        let lifetime = self.signed(token, &header, true).is_some();
        let scopes = self.scoped(&claims);
        let passes = issuer && audience && lifetime && scopes;
        let granted = passes.then(|| self.granted(&claims));
        Checked {
            signature: true,
            issuer: Some(issuer),
            audience: Some(audience),
            lifetime: Some(lifetime),
            scopes: Some(scopes),
            grant: granted.as_ref().map(Granted::union),
            sources: granted.and_then(|g| g.github.map(|github| (g.claims, github))),
        }
    }

    /// The state an operator explains a 401 or a 403 with (ADR 0014): the identity provider's
    /// `type` (ADR 0016), the issuer, its metadata, the JWKS cache (the cached key ids, the last
    /// read, a refresh that failed since, with why) and the groups table, each group's identities
    /// those `reads` admits (`*` kept: it names none); with `[oidc.github]`, `github`: the
    /// provider, the grant's state and the newest set as loaded, its counts the job's (#332).
    #[cfg(feature = "ui")]
    pub fn admission(&self, reads: &dyn Fn(&str) -> bool) -> Value {
        let kids: Vec<Option<String>> = (self.keys().iter())
            .map(|key| key.common.key_id.clone())
            .collect();
        let fetched = self.fetched.lock().unwrap_or_else(|e| e.into_inner());
        let failed = (fetched.failed.as_ref()).map(
            |(at, why)| serde_json::json!({"time": crate::modules::rfc3339(*at), "reason": why}),
        );
        let groups: Option<Map<String, Value>> = (self.config.groups.as_ref()).map(|(_, table)| {
            (table.iter())
                .map(|(group, identities)| {
                    let shown = identities.iter().filter(|i| *i == "*" || reads(i));
                    (group.clone(), shown.cloned().collect::<Vec<_>>().into())
                })
                .collect()
        });
        #[allow(unused_mut)]
        let mut document = serde_json::json!({
            "type": self.config.kind,
            "issuer": self.config.issuer,
            "metadata": self.issuer_metadata,
            "jwks_uri": self.jwks_uri,
            "keys": kids,
            "refreshed": crate::modules::rfc3339(fetched.at),
            "failed": failed,
            "identities_claim": self.config.claim,
            "groups_claim": self.config.groups.as_ref().map(|(claim, _)| claim),
            "groups": groups,
            "scopes": self.config.scopes,
        });
        #[cfg(feature = "postgres")]
        if let Some(cache) = &self.permissions {
            document["github"] = cache.admission();
        }
        document
    }

    /// The issuer and the scopes the console's browser logs in with (ADR 0014).
    #[cfg(feature = "ui")]
    pub fn login(&self) -> (&str, &[String]) {
        (&self.config.issuer, &self.config.scopes)
    }

    fn keys(&self) -> Arc<Vec<Jwk>> {
        self.keys.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Fetches the JWKS for a `kid` it lacks, unless an unknown `kid` already did within
    /// `KID_REFRESH_EVERY`. The lock only claims the fetch: a request arriving during it does
    /// not wait, and is checked against the keys cached then.
    fn refetch(&self) {
        {
            let mut fetched = self.kid_fetched.lock().unwrap_or_else(|e| e.into_inner());
            if fetched.is_some_and(|at| at.elapsed() < KID_REFRESH_EVERY) {
                return;
            }
            *fetched = Some(Instant::now());
        }
        self.refresh();
    }

    /// Fetches the JWKS again: the new keys replace the cached ones, and a failure keeps them
    /// and says so on stderr.
    fn refresh(&self) {
        let fetched = jwks(&self.agent, &self.jwks_uri);
        let mut state = self.fetched.lock().unwrap_or_else(|e| e.into_inner());
        let now = crate::modules::now();
        match fetched {
            Ok(keys) => {
                *self.keys.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(keys);
                *state = Fetched {
                    at: now,
                    failed: None,
                };
            }
            Err(why) => {
                eprintln!("{}", Msg::OidcRefreshFailed(&why).text());
                state.failed = Some((now, why));
            }
        }
    }
}

/// The refresh thread: fetches the JWKS of `module` every `every`, until the module is gone.
fn refresh_every(module: Weak<Oidc>, every: Duration) {
    loop {
        std::thread::sleep(every);
        let Some(module) = module.upgrade() else {
            return;
        };
        module.refresh();
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use jsonwebtoken::EncodingKey;

    use super::*;

    /// The section with `issuer`, `public_url` and `rest`.
    fn bare(rest: &str) -> Result<Config, crate::modules::Problem> {
        let src = format!(
            "issuer = \"https://id.example/realms/agents\"\npublic_url = \
             \"https://meada.example/\"\n{rest}"
        );
        toml::from_str::<File>(&src).unwrap().validate()
    }

    fn section(extra: &str) -> Result<Config, crate::modules::Problem> {
        bare(&format!(
            "identities_claim = \"meada_repositories\"\n{extra}"
        ))
    }

    #[test]
    fn the_github_table_takes_its_defaults_and_names_each_bad_key() {
        let table = |rest: &str| format!("[github]\nlogin_claim = \"github_login\"\n{rest}");
        let parsed = bare(&table("max_age_seconds = 5400\n"));
        if !cfg!(feature = "postgres") {
            // Without the data layer the table is the error naming the feature.
            assert_eq!(parsed.err().map(|e| e.0), Some("oidc.github"));
            return;
        }
        // Alone, with neither `identities_claim` nor `groups_claim`.
        let github = parsed.unwrap().github.unwrap();
        assert_eq!(github.host, "github.com");
        assert_eq!(github.login_claim, "github_login");
        assert_eq!(github.roles, ROLES);
        assert_eq!(github.max_age, Duration::from_secs(5400));
        let custom = "host = \"ghe.example\"\nroles = [\"Custom Role\", \"read\"]\n\
                      max_age_seconds = 60\n";
        let github = bare(&table(custom)).unwrap().github.unwrap();
        assert_eq!(github.host, "ghe.example");
        assert_eq!(github.roles, ["Custom Role", "read"]);
        let long = format!("roles = [\"{}\"]\nmax_age_seconds = 60\n", "r".repeat(101));
        for (rest, key) in [
            ("max_age_seconds = 59\n", "oidc.github.max_age_seconds"),
            ("", "oidc.github"),
            (
                "host = \"GitHub.com\"\nmax_age_seconds = 60\n",
                "oidc.github.host",
            ),
            ("host = \"\"\nmax_age_seconds = 60\n", "oidc.github.host"),
            (
                "host = \"github.com:443\"\nmax_age_seconds = 60\n",
                "oidc.github.host",
            ),
            ("roles = []\nmax_age_seconds = 60\n", "oidc.github.roles"),
            (
                "roles = [\"read\", \"read\"]\nmax_age_seconds = 60\n",
                "oidc.github.roles",
            ),
            (
                "roles = [\"\"]\nmax_age_seconds = 60\n",
                "oidc.github.roles",
            ),
            (
                "roles = [\"re\\nad\"]\nmax_age_seconds = 60\n",
                "oidc.github.roles",
            ),
            (&long, "oidc.github.roles"),
        ] {
            assert_eq!(bare(&table(rest)).err().map(|e| e.0), Some(key), "{rest}");
        }
        let claimless = "[github]\nmax_age_seconds = 60\n";
        assert_eq!(bare(claimless).err().map(|e| e.0), Some("oidc.github"));
        let empty = "[github]\nlogin_claim = \"\"\nmax_age_seconds = 60\n";
        assert_eq!(
            bare(empty).err().map(|e| e.0),
            Some("oidc.github.login_claim")
        );
        let at_most = format!("roles = [\"{}\"]\nmax_age_seconds = 60\n", "é".repeat(100));
        assert!(bare(&table(&at_most)).is_ok());
    }

    #[test]
    fn overage_through_the_directory_needs_the_groups_claim_and_the_feature() {
        let table = |kind: &str, groups: bool, overage: &str| {
            let groups = match groups {
                true => "groups_claim = \"groups\"\n[groups]\n\"g\" = [\"a\"]\n",
                false => "",
            };
            bare(&format!(
                "identities_claim = \"c\"\ntype = \"{kind}\"\n{groups}[entra]\n{overage}\n"
            ))
        };
        let directory = "overage = \"directory\"";
        let ignored = table("entra", false, "overage = \"ignore\"").unwrap();
        assert_eq!(ignored.overage, None);
        assert_eq!(table("entra", true, "").unwrap().overage, None);
        let key = "oidc.entra.overage";
        if cfg!(feature = "directory") {
            assert!(table("entra", true, directory).unwrap().overage.is_some());
            let missing = Msg::KeyMissing("oidc.groups_claim").text();
            assert_eq!(table("entra", false, directory), Err((key, missing)));
        } else {
            // Without the module the key names the feature, with or without the claim.
            for groups in [true, false] {
                let refused = table("entra", groups, directory).unwrap_err();
                let (section, feature) = ("oidc.entra", "directory");
                let named = Msg::NeedsFeature { section, feature }.text();
                assert_eq!(refused, (key, named));
            }
        }
        // Under another `type` the error of `[oidc.entra]` stands.
        let other = table("okta", true, directory).unwrap_err();
        assert_eq!(other, ("oidc.entra", Msg::OidcTypeTable("entra").text()));
        let always = table("entra", true, "overage = \"always\"").unwrap_err();
        assert_eq!(always.0, key);
    }

    #[test]
    fn the_claims_and_the_groups_table_are_checked_together() {
        let groups = "[groups]\n\"team-a\" = [\"a\"]\n";
        let config = bare(&format!("groups_claim = \"groups\"\n{groups}")).unwrap();
        assert_eq!(config.claim, None);
        let table = HashMap::from([("team-a".to_string(), vec!["a".to_string()])]);
        assert_eq!(config.groups, Some(("groups".to_string(), table)));
        let key = |rest: &str| bare(rest).unwrap_err();
        let missing = |key: &str| Msg::KeyMissing(key).text();
        assert_eq!(
            key("groups_claim = \"groups\"\n"),
            ("oidc.groups_claim", missing("oidc.groups"))
        );
        assert_eq!(key(groups), ("oidc.groups", missing("oidc.groups_claim")));
        assert_eq!(
            key(&format!("groups_claim = \"\"\n{groups}")),
            ("oidc.groups_claim", Msg::OidcEmpty.text())
        );
        assert_eq!(
            key("identities_claim = \"\"\n"),
            ("oidc.identities_claim", Msg::OidcEmpty.text())
        );
        assert_eq!(key(""), ("oidc.identities_claim", Msg::OidcNoClaim.text()));
    }

    /// What `claims` grants under `[oidc.groups]` mapping `g` to `b` and `p` to `*`: `None` for
    /// all, else the identities, sorted.
    fn granted(claims: Value) -> Option<Vec<String>> {
        let table = HashMap::from([
            ("g".to_string(), vec!["b".to_string()]),
            ("p".to_string(), vec!["*".to_string()]),
        ]);
        let config = Config {
            groups: Some(("groups".into(), table)),
            ..config(None, &[])
        };
        let claims = claims.as_object().unwrap();
        let groups: Vec<String> = strings(claims, "groups").map(str::to_string).collect();
        match Grant::of(claims, &config, &groups) {
            Grant::All => None,
            Grant::Only(identities) => {
                let mut identities: Vec<String> = identities.into_iter().collect();
                identities.sort();
                Some(identities)
            }
        }
    }

    #[test]
    fn a_grant_is_the_union_of_the_identities_and_what_the_groups_map_to() {
        use serde_json::json;
        let only = |identities: &[&str]| Some(identities.iter().map(|s| s.to_string()).collect());
        assert_eq!(granted(json!({"groups": ["g"]})), only(&["b"]));
        assert_eq!(granted(json!({"meada_repositories": ["a"]})), only(&["a"]));
        let both = json!({"meada_repositories": ["a"], "groups": ["g"]});
        assert_eq!(granted(both), only(&["a", "b"]));
        assert_eq!(granted(json!({"groups": ["g", "p"]})), None);
        assert_eq!(granted(json!({"groups": ["unmapped"]})), only(&[]));
        // Not an array of strings grants nothing, even through its string elements.
        let shapes = json!({"meada_repositories": "a", "groups": [{"g": 1}, 7, "g"]});
        assert_eq!(granted(shapes), only(&[]));
        assert_eq!(granted(json!({"meada_repositories": ["a", 7]})), only(&[]));
        assert_eq!(granted(json!({"groups": "g"})), only(&[]));
        assert_eq!(granted(json!({})), only(&[]));
    }

    #[test]
    fn the_section_takes_https_urls_or_loopback_and_defaults_the_rest() {
        let config = section("").unwrap();
        assert_eq!(config.public_url, "https://meada.example");
        assert_eq!(config.refresh, Duration::from_secs(3600));
        assert_eq!(
            (config.audience, config.scopes),
            (None, Vec::<String>::new())
        );
        for good in [
            "https://id.example",
            "http://localhost:8080/realms/x",
            "http://127.0.0.1:9",
            "http://[::1]:9",
        ] {
            assert!(reachable(good, Shape::Path), "{good}");
        }
        for bad in [
            "http://id.example",
            "https://user:pw@id.example",
            "https://id.example/?q=1",
            "https://id.example/#f",
            "ftp://id.example",
            "id.example",
            "",
            // Names that only look like loopback, and an IPv4-mapped loopback address.
            "http://localhost.evil.com",
            "http://127.0.0.1.nip.io",
            "http://[::ffff:127.0.0.1]",
            "http://localhost:80@evil.com",
        ] {
            assert!(!reachable(bad, Shape::Path), "{bad}");
        }
        assert!(!reachable("https://meada.example/prefix", Shape::Bare));
        // A `jwks_uri` may carry a query, never a user or a fragment.
        assert!(reachable("https://id.example/jwks?realm=a", Shape::Query));
        for bad in [
            "https://u@id.example/jwks?realm=a",
            "https://id.example/jwks?realm=a#f",
            "http://id.example/jwks?realm=a",
        ] {
            assert!(!reachable(bad, Shape::Query), "{bad}");
        }
        let key = |extra: &str| section(extra).unwrap_err().0;
        assert_eq!(
            key("jwks_refresh_seconds = 59"),
            "oidc.jwks_refresh_seconds"
        );
        assert_eq!(key("audience = \"\""), "oidc.audience");
        let scope = |extra: &str| section(extra).unwrap_err().1;
        assert_eq!(
            scope("scopes = [\"a b\"]"),
            "\"a b\" holds ' ', which a scope cannot carry (RFC 6749)"
        );
        assert_eq!(
            scope("scopes = [\"a\\\"\"]"),
            "\"a\\\"\" holds '\"', which a scope cannot carry (RFC 6749)"
        );
        assert_eq!(scope("scopes = [\"\"]"), "must not be empty");
        assert_eq!(
            section("scopes = [\"meada:read\"]").unwrap().scopes,
            ["meada:read"]
        );
        let plain = "issuer = \"http://id.example\"\npublic_url = \"https://m.example\"\n\
                     identities_claim = \"c\"\n";
        let err = toml::from_str::<File>(plain)
            .unwrap()
            .validate()
            .unwrap_err();
        assert_eq!(err.0, "oidc.issuer");
    }

    #[test]
    fn metadata_is_looked_for_where_the_mcp_specification_says_in_its_order() {
        assert_eq!(
            metadata_urls("https://id.example/realms/agents/"),
            [
                "https://id.example/.well-known/oauth-authorization-server/realms/agents",
                "https://id.example/.well-known/openid-configuration/realms/agents",
                "https://id.example/realms/agents/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            metadata_urls("http://127.0.0.1:9"),
            [
                "http://127.0.0.1:9/.well-known/oauth-authorization-server",
                "http://127.0.0.1:9/.well-known/openid-configuration",
            ]
        );
    }

    /// A local HTTP server: each request gets `answer(path)`, a status, more header lines and a
    /// body. Its base URL and the count of requests it read.
    fn issuer(
        answer: impl Fn(&str, &str) -> (u16, String, String) + Send + 'static,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let (counted, at) = (hits.clone(), base.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let path = line.split(' ').nth(1).unwrap_or("").to_string();
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    header.clear();
                }
                counted.fetch_add(1, Ordering::SeqCst);
                let (status, headers, body) = answer(&at, &path);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\
                     {headers}\r\n{body}",
                    body.len()
                );
            }
        });
        (base, hits)
    }

    /// A P-256 key generated now, as a signing key and its public JWK with `kid`.
    fn key(kid: &str) -> (EncodingKey, Jwk) {
        let rng = ring::rand::SystemRandom::new();
        let alg = &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING;
        let pkcs8 = ring::signature::EcdsaKeyPair::generate_pkcs8(alg, &rng).unwrap();
        let signing = EncodingKey::from_ec_der(pkcs8.as_ref());
        let mut jwk = Jwk::from_encoding_key(&signing, Algorithm::ES256).unwrap();
        jwk.common.key_id = Some(kid.into());
        (signing, jwk)
    }

    fn set(keys: &[&Jwk]) -> String {
        serde_json::json!({ "keys": keys }).to_string()
    }

    fn now() -> u64 {
        jsonwebtoken::get_current_timestamp()
    }

    const ISSUER: &str = "https://id.example";
    const PUBLIC: &str = "https://meada.example";

    fn module(config: Config, keys: Vec<Jwk>) -> Oidc {
        Oidc::new(
            config,
            agent(FETCH_TIMEOUT),
            "http://127.0.0.1:9/jwks".into(),
            keys,
        )
    }

    fn config(audience: Option<&str>, scopes: &[&str]) -> Config {
        Config {
            kind: "generic",
            issuer: ISSUER.into(),
            public_url: PUBLIC.into(),
            audience: audience.map(str::to_string),
            claim: Some("meada_repositories".into()),
            groups: None,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            refresh: Duration::from_secs(3600),
            github: None,
            overage: None,
        }
    }

    /// The claims of a token that passes for `/a`, changed by `edit`.
    fn claims(edit: impl FnOnce(&mut Value)) -> Value {
        let mut claims = serde_json::json!({
            "iss": ISSUER, "aud": format!("{PUBLIC}/a"), "exp": now() + 300,
            "meada_repositories": ["a"], "scope": "meada:read other",
        });
        edit(&mut claims);
        claims
    }

    fn sign(key: &EncodingKey, kid: &str, alg: Algorithm, claims: &Value) -> String {
        let mut header = Header::new(alg);
        header.kid = Some(kid.into());
        jsonwebtoken::encode(&header, claims, key).unwrap()
    }

    fn headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let value = format!("Bearer {token}");
        headers.insert(header::AUTHORIZATION, value.parse().unwrap());
        headers
    }

    /// What `oidc` answers a request to `path` (identity `identity`) with `token`: the status
    /// of a refusal and its challenge, or the grant's answer about `a` and `b`.
    fn admitted(
        oidc: &Oidc,
        token: &str,
        path: &str,
        identity: &str,
    ) -> Result<(bool, bool), (u16, String)> {
        match oidc.admit(&headers(token), path, identity) {
            Ok(grant) => Ok((grant.reads("a"), grant.reads("b"))),
            Err(reply) => {
                let challenge = reply.headers().get(header::WWW_AUTHENTICATE);
                let challenge = challenge.map_or("", |v| v.to_str().unwrap()).to_string();
                Err((reply.status().as_u16(), challenge))
            }
        }
    }

    #[test]
    fn a_token_passes_only_signed_by_a_fitting_key_issued_addressed_and_in_time() {
        let (signing, jwk) = key("k1");
        let (other, _) = key("k1");
        let oidc = module(config(None, &[]), vec![jwk]);
        let ok = |claims: &Value| sign(&signing, "k1", Algorithm::ES256, claims);
        let pass = |token: &str| admitted(&oidc, token, "a", "a");
        let invalid = Err((
            401,
            "Bearer error=\"invalid_token\", resource_metadata=\"https://meada.example/.well-known/oauth-protected-resource/a\"".to_string(),
        ));
        assert_eq!(pass(&ok(&claims(|_| {}))), Ok((true, false)));
        // The audience as an array, and a pinned path, which takes `/a`'s audience and its own.
        let array = ok(&claims(|c| {
            c["aud"] = serde_json::json!(["x", format!("{PUBLIC}/a")])
        }));
        assert_eq!(pass(&array), Ok((true, false)));
        let pin = "a@0123456789abcdef";
        assert_eq!(
            admitted(&oidc, &ok(&claims(|_| {})), pin, "a"),
            Ok((true, false))
        );
        let pinned = ok(&claims(|c| c["aud"] = format!("{PUBLIC}/{pin}").into()));
        assert_eq!(admitted(&oidc, &pinned, pin, "a"), Ok((true, false)));
        // Within the leeway, an expiry just past and a start just ahead pass.
        let skewed = ok(&claims(|c| {
            c["exp"] = (now() - LEEWAY / 2).into();
            c["nbf"] = (now() + LEEWAY / 2).into();
        }));
        assert_eq!(pass(&skewed), Ok((true, false)));

        for (case, token) in [
            (
                "another key",
                sign(&other, "k1", Algorithm::ES256, &claims(|_| {})),
            ),
            (
                "expired",
                ok(&claims(|c| c["exp"] = (now() - 2 * LEEWAY).into())),
            ),
            (
                "no exp",
                ok(&claims(|c| drop(c.as_object_mut().unwrap().remove("exp")))),
            ),
            ("exp a string", ok(&claims(|c| c["exp"] = "never".into()))),
            (
                "not yet",
                ok(&claims(|c| c["nbf"] = (now() + 2 * LEEWAY).into())),
            ),
            (
                "wrong issuer",
                ok(&claims(|c| c["iss"] = "https://other.example".into())),
            ),
            (
                "no issuer",
                ok(&claims(|c| drop(c.as_object_mut().unwrap().remove("iss")))),
            ),
            (
                "issuer list",
                ok(&claims(|c| c["iss"] = serde_json::json!([ISSUER]))),
            ),
            (
                "wrong audience",
                ok(&claims(|c| c["aud"] = format!("{PUBLIC}/b").into())),
            ),
            (
                "no audience",
                ok(&claims(|c| drop(c.as_object_mut().unwrap().remove("aud")))),
            ),
            ("malformed", "a.b".to_string()),
            ("not base64", "*.*.*".to_string()),
        ] {
            assert_eq!(pass(&token), invalid, "{case}");
        }
        // `/b`'s audience is no audience of `/a`, nor `/a`'s of `/b`.
        let b = Err((
            401,
            "Bearer error=\"invalid_token\", resource_metadata=\"https://meada.example/.well-known/oauth-protected-resource/b\"".to_string(),
        ));
        assert_eq!(admitted(&oidc, &ok(&claims(|_| {})), "b", "b"), b);
    }

    /// `alg: none`, HMAC (with the public key's bytes as the secret, the classic confusion) and
    /// an `alg` the key does not name are refused.
    #[test]
    fn none_hmac_and_an_algorithm_the_key_does_not_fit_are_refused() {
        let (signing, jwk) = key("k1");
        let oidc = module(config(None, &[]), vec![jwk.clone()]);
        let good = sign(&signing, "k1", Algorithm::ES256, &claims(|_| {}));
        let (_, payload) = good.split_once('.').unwrap();
        let payload = payload.rsplit_once('.').unwrap().0;
        // {"alg":"none","typ":"JWT","kid":"k1"}
        let none = format!("eyJhbGciOiJub25lIiwidHlwIjoiSldUIiwia2lkIjoiazEifQ.{payload}.");
        let secret = serde_json::to_vec(&jwk).unwrap();
        let hmac = sign(
            &EncodingKey::from_secret(&secret),
            "k1",
            Algorithm::HS256,
            &claims(|_| {}),
        );
        // A P-384 key signing under the P-256 key's `kid`: ES384 does not fit that key.
        let rng = ring::rand::SystemRandom::new();
        let alg = &ring::signature::ECDSA_P384_SHA384_FIXED_SIGNING;
        let pkcs8 = ring::signature::EcdsaKeyPair::generate_pkcs8(alg, &rng).unwrap();
        let p384 = EncodingKey::from_ec_der(pkcs8.as_ref());
        let es384 = sign(&p384, "k1", Algorithm::ES384, &claims(|_| {}));
        for (case, token) in [("none", none), ("hmac", hmac), ("es384", es384)] {
            assert_eq!(
                admitted(&oidc, &token, "a", "a").unwrap_err().0,
                401,
                "{case}"
            );
        }
        // An RSA key fits RS and PS algorithms only; an EC key, its own curve only.
        let rsa: Jwk = serde_json::from_value(serde_json::json!({
            "kty": "RSA", "kid": "r", "n": "AQAB", "e": "AQAB", "alg": "RS256"
        }))
        .unwrap();
        let header = |alg| {
            let mut header = Header::new(alg);
            header.kid = Some("r".into());
            header
        };
        assert!(fits(&rsa, &header(Algorithm::RS256)));
        assert!(
            !fits(&rsa, &header(Algorithm::PS256)),
            "the key names RS256"
        );
        assert!(!fits(&rsa, &header(Algorithm::ES256)));
        assert!(fits(&jwk, &Header::new(Algorithm::ES256)));
        assert!(!fits(&jwk, &Header::new(Algorithm::ES384)));
        // An HMAC key or an encryption key is no signing key.
        let oct: Jwk =
            serde_json::from_value(serde_json::json!({"kty": "oct", "k": "c2VjcmV0"})).unwrap();
        assert!(!signs(&oct));
        let mut enc = jwk.clone();
        enc.common.public_key_use = Some(PublicKeyUse::Encryption);
        assert!(!signs(&enc) && signs(&jwk));
    }

    #[test]
    fn the_configured_audience_scopes_and_the_claim_decide_the_rest() {
        let (signing, jwk) = key("k1");
        let oidc = module(config(Some("api://meada"), &["meada:read"]), vec![jwk]);
        let token = |edit: fn(&mut Value)| {
            sign(
                &signing,
                "k1",
                Algorithm::ES256,
                &claims(|c| {
                    c["aud"] = "api://meada".into();
                    edit(c);
                }),
            )
        };
        assert_eq!(admitted(&oidc, &token(|_| {}), "a", "a"), Ok((true, false)));
        // The configured audience replaces the endpoint's.
        let endpoint = token(|c| c["aud"] = format!("{PUBLIC}/a").into());
        assert_eq!(admitted(&oidc, &endpoint, "a", "a").unwrap_err().0, 401);
        let all = token(|c| c["meada_repositories"] = serde_json::json!(["*"]));
        assert_eq!(admitted(&oidc, &all, "a", "a"), Ok((true, true)));
        // A string, or no claim, grants nothing; the token is still valid.
        let string = token(|c| c["meada_repositories"] = "a".into());
        assert_eq!(admitted(&oidc, &string, "a", "a"), Ok((false, false)));
        let scoped = Err((
            403,
            "Bearer error=\"insufficient_scope\", resource_metadata=\"https://meada.example/.well-known/oauth-protected-resource/a\", scope=\"meada:read\"".to_string(),
        ));
        assert_eq!(
            admitted(&oidc, &token(|c| c["scope"] = "other".into()), "a", "a"),
            scoped
        );
        assert_eq!(
            admitted(
                &oidc,
                &token(|c| drop(c.as_object_mut().unwrap().remove("scope"))),
                "a",
                "a"
            ),
            scoped
        );
    }

    #[test]
    fn no_token_another_scheme_and_origin_get_their_answers_and_metadata_needs_no_token() {
        let oidc = module(config(None, &["meada:read"]), Vec::new());
        let answer = |headers: HeaderMap, path: &str| {
            let reply = oidc.admit(&headers, path, path).err().unwrap();
            let challenge = (reply.headers().get(header::WWW_AUTHENTICATE))
                .map(|v| v.to_str().unwrap().to_string());
            (reply.status().as_u16(), challenge)
        };
        let metadata = "https://meada.example/.well-known/oauth-protected-resource";
        assert_eq!(
            answer(HeaderMap::new(), "a b\"c"),
            (
                401,
                Some(format!(
                    "Bearer resource_metadata=\"{metadata}/a%20b%22c\", scope=\"meada:read\""
                ))
            )
        );
        assert_eq!(
            answer(HeaderMap::new(), ""),
            (
                401,
                Some(format!(
                    "Bearer resource_metadata=\"{metadata}\", scope=\"meada:read\""
                ))
            )
        );
        // A CR LF in the path stays encoded: the challenge is one header.
        assert_eq!(
            answer(HeaderMap::new(), "a\r\nX-Evil: 1").1.unwrap(),
            format!(
                "Bearer resource_metadata=\"{metadata}/a%0D%0AX-Evil:%201\", scope=\"meada:read\""
            )
        );
        // Another scheme is no bearer token (RFC 6750 §3.1); an empty bearer, two headers or
        // a value that is not UTF-8 are refused.
        let with = |values: &[&[u8]]| {
            let mut headers = HeaderMap::new();
            for value in values {
                let value = header::HeaderValue::from_bytes(value).unwrap();
                headers.append(header::AUTHORIZATION, value);
            }
            answer(headers, "a").1.unwrap()
        };
        let none = format!("Bearer resource_metadata=\"{metadata}/a\", scope=\"meada:read\"");
        let refused = format!(
            "Bearer error=\"invalid_token\", resource_metadata=\"{metadata}/a\", scope=\"meada:read\""
        );
        assert_eq!(with(&[b"Basic YTpi"]), none);
        assert_eq!(with(&[b"Basic"]), none);
        assert_eq!(with(&[b"Bearer "]), refused);
        assert_eq!(with(&[b"Bearer a.b.c", b"Bearer a.b.c"]), refused);
        assert_eq!(with(&[b"Bearer a.b.\xff"]), refused);
        // The metadata of a path and of a pinned one.
        for (path, resource) in [
            (format!("{METADATA_PATH}/a"), "https://meada.example/a"),
            (
                format!("{METADATA_PATH}/a@0123456789abcdef"),
                "https://meada.example/a@0123456789abcdef",
            ),
            (METADATA_PATH.to_string(), "https://meada.example"),
        ] {
            let reply = oidc.admit(&HeaderMap::new(), &path, &path).err().unwrap();
            assert_eq!(reply.status(), 200);
            assert_eq!(reply.headers()[header::CONTENT_TYPE], "application/json");
            let body = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(reply.into_body().collect())
                .unwrap()
                .to_bytes();
            let json: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                json,
                serde_json::json!({
                    "resource": resource, "authorization_servers": [ISSUER],
                    "bearer_methods_supported": ["header"], "scopes_supported": ["meada:read"],
                }),
                "{path}"
            );
        }
        // `.well-known/oauth-protected-resourcex` is no metadata path.
        assert_eq!(
            answer(HeaderMap::new(), &format!("{METADATA_PATH}x")).0,
            401
        );
    }

    #[test]
    /// The RFC 8414 document answers 200 with junk, so OpenID discovery's is used; its
    /// `jwks_uri` carries a query.
    fn start_reads_the_metadata_the_issuer_names_and_its_jwks() {
        let (_, jwk) = key("k1");
        let keys = set(&[&jwk]);
        let (base, _) = issuer(move |base, path| match path {
            "/.well-known/oauth-authorization-server" => {
                (200, String::new(), "<html>sign in</html>".into())
            }
            "/.well-known/openid-configuration" => (
                200,
                String::new(),
                serde_json::json!({"issuer": base, "jwks_uri": format!("{base}/jwks?realm=a")})
                    .to_string(),
            ),
            "/jwks?realm=a" => (200, String::new(), keys.clone()),
            _ => (404, String::new(), String::new()),
        });
        let config = Config {
            issuer: base.clone(),
            ..config(None, &[])
        };
        let oidc = Oidc::start(config).unwrap();
        assert_eq!(oidc.jwks_uri, format!("{base}/jwks?realm=a"));
        assert_eq!(oidc.keys().len(), 1);
    }

    /// A document naming another issuer is passed over too, and the next one is used.
    #[test]
    fn a_document_naming_another_issuer_is_passed_over() {
        let (_, jwk) = key("k1");
        let keys = set(&[&jwk]);
        let (base, _) = issuer(move |base, path| {
            match path {
            "/.well-known/oauth-authorization-server" => (
                200,
                String::new(),
                serde_json::json!({"issuer": "https://other.example", "jwks_uri": "https://other.example/jwks"})
                    .to_string(),
            ),
            "/.well-known/openid-configuration" => (
                200,
                String::new(),
                serde_json::json!({"issuer": base, "jwks_uri": format!("{base}/jwks")}).to_string(),
            ),
            "/jwks" => (200, String::new(), keys.clone()),
            _ => (404, String::new(), String::new()),
        }
        });
        let config = Config {
            issuer: base.clone(),
            ..config(None, &[])
        };
        assert_eq!(
            Oidc::start(config).unwrap().jwks_uri,
            format!("{base}/jwks")
        );
    }

    #[test]
    fn start_refuses_a_redirect_another_issuer_a_plain_jwks_uri_and_no_metadata() {
        let (elsewhere, elsewhere_hits) = issuer(|base, _| {
            (
                200,
                String::new(),
                serde_json::json!({"issuer": base}).to_string(),
            )
        });
        let start = |answer: fn(&str, &str, &str) -> (u16, String, String)| {
            let to = elsewhere.clone();
            let (base, _) = issuer(move |base, path| answer(base, path, &to));
            let config = Config {
                issuer: base,
                ..config(None, &[])
            };
            Oidc::start(config).err().unwrap().to_string()
        };
        let moved = start(|_, _, to| (302, format!("Location: {to}/\r\n"), String::new()));
        assert!(moved.contains("http status 302"), "{moved}");
        assert_eq!(
            elsewhere_hits.load(Ordering::SeqCst),
            0,
            "a redirect was followed"
        );
        let other = start(|_, _, to| {
            (
                200,
                String::new(),
                serde_json::json!({"issuer": to}).to_string(),
            )
        });
        assert!(other.contains("not the configured one"), "{other}");
        let plain = start(|base, _, _| {
            let doc = serde_json::json!({"issuer": base, "jwks_uri": "http://id.example/jwks"});
            (200, String::new(), doc.to_string())
        });
        assert!(plain.contains("jwks_uri"), "{plain}");
        let none = start(|_, _, _| (404, String::new(), String::new()));
        assert!(
            none.contains("oauth-authorization-server: http status 404"),
            "{none}"
        );
        assert!(
            none.contains("openid-configuration: http status 404"),
            "{none}"
        );
    }

    #[test]
    fn a_document_or_a_jwks_past_its_cap_fails_and_so_does_a_silent_issuer() {
        let (base, _) = issuer(|_, path| {
            let size = match path {
                "/metadata" => METADATA_MAX + 1,
                "/exact" => METADATA_MAX,
                _ => JWKS_MAX + 1,
            };
            (200, String::new(), " ".repeat(size as usize))
        });
        let agent = agent(FETCH_TIMEOUT);
        // The cap is inclusive.
        let exact = get(&agent, &format!("{base}/exact"), METADATA_MAX).unwrap();
        assert_eq!(exact.len() as u64, METADATA_MAX);
        let big = get(&agent, &format!("{base}/metadata"), METADATA_MAX).unwrap_err();
        assert!(big.contains("limit"), "{big}");
        let big = jwks(&agent, &format!("{base}/jwks")).unwrap_err();
        assert!(big.contains("limit"), "{big}");
        // An issuer that accepts and never answers: the request ends at its timeout.
        let silent = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", silent.local_addr().unwrap());
        let started = Instant::now();
        let slow = get(&self::agent(Duration::from_millis(200)), &url, METADATA_MAX).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5), "{slow}");
        assert!(slow.contains("timeout"), "{slow}");
        drop(silent);
    }

    /// A rotated key reaches the module on the first token naming its `kid`; a second unknown
    /// `kid` within `KID_REFRESH_EVERY` fetches nothing; a failed fetch keeps the keys.
    #[test]
    fn an_unknown_kid_fetches_the_jwks_once_per_interval() {
        let (old, old_jwk) = key("old");
        let (new, new_jwk) = key("new");
        let rotated = set(&[&old_jwk, &new_jwk]);
        let (base, hits) = issuer(move |_, _| (200, String::new(), rotated.clone()));
        let mut oidc = module(config(None, &[]), vec![old_jwk]);
        oidc.jwks_uri = format!("{base}/jwks");
        let token =
            |key: &EncodingKey, kid: &str| sign(key, kid, Algorithm::ES256, &claims(|_| {}));
        assert_eq!(
            admitted(&oidc, &token(&old, "old"), "a", "a"),
            Ok((true, false))
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert_eq!(
            admitted(&oidc, &token(&new, "new"), "a", "a"),
            Ok((true, false))
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let (stray, _) = key("stray");
        assert_eq!(
            admitted(&oidc, &token(&stray, "stray"), "a", "a")
                .unwrap_err()
                .0,
            401
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a second fetch within the interval"
        );
        // A failed refresh keeps the cached keys.
        oidc.jwks_uri = "http://127.0.0.1:9/jwks".into();
        oidc.refresh();
        assert_eq!(oidc.keys().len(), 2);
    }

    /// While an unknown `kid` fetches the JWKS, another one does not wait for the fetch: the
    /// lock is never held across the network.
    #[test]
    fn a_fetch_in_flight_holds_no_request_back() {
        let (_, jwk) = key("k1");
        let keys = set(&[&jwk]);
        let (base, hits) = issuer(move |_, _| {
            std::thread::sleep(Duration::from_secs(2));
            (200, String::new(), keys.clone())
        });
        let mut oidc = module(config(None, &[]), vec![jwk]);
        oidc.jwks_uri = format!("{base}/jwks");
        let oidc = Arc::new(oidc);
        let (stray, _) = key("stray");
        let token = sign(&stray, "stray", Algorithm::ES256, &claims(|_| {}));
        let fetching = {
            let (oidc, token) = (oidc.clone(), token.clone());
            std::thread::spawn(move || admitted(&oidc, &token, "a", "a").unwrap_err().0)
        };
        while hits.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
        let started = Instant::now();
        assert_eq!(admitted(&oidc, &token, "a", "a").unwrap_err().0, 401);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(fetching.join().unwrap(), 401);
    }

    /// A JWK with no `kid` fits a token naming one; a JWK with a `kid` fits only that one; a
    /// token with `crit` is refused.
    #[test]
    fn a_key_without_a_kid_fits_and_crit_is_refused() {
        let (signing, mut jwk) = key("k1");
        jwk.common.key_id = None;
        let oidc = module(config(None, &[]), vec![jwk]);
        let token = sign(&signing, "any", Algorithm::ES256, &claims(|_| {}));
        assert_eq!(admitted(&oidc, &token, "a", "a"), Ok((true, false)));
        let mut named = Header::new(Algorithm::ES256);
        named.kid = Some("k1".into());
        let (_, kid) = key("k2");
        assert!(!fits(&kid, &named));
        let mut critical = Header::new(Algorithm::ES256);
        critical.crit = Some(vec!["exp".into()]);
        let token = jsonwebtoken::encode(&critical, &claims(|_| {}), &signing).unwrap();
        assert_eq!(admitted(&oidc, &token, "a", "a").unwrap_err().0, 401);
    }
}
