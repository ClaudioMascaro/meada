//! Local accounts (ADR 0024, feature `accounts`): usernames and passwords in the data layer, and
//! the session tokens the console signs with Ed25519 for them. The `accounts` module of ADR 0012's
//! table, section `[accounts]`, which needs `[postgres]` and sits beside `[oidc]` only with
//! `break_glass = true` (the break-glass account, #563).
//!
//! `File` is the section and `File::validate` its `Config`: where the signing key's public and
//! private halves and the default account's initial password come from, each a variable or a
//! file (at most one of each pair), never a value, and whether it is break glass.
//! `Manifest::read` checks the section's needs (`under_break_glass` before the section is read)
//! and gives it its `[postgres]`. Only the console reads the keys (`Config::keys`), once, at start:
//! a missing, unreadable, non-Ed25519 or mismatched pair refuses the start, naming the key and
//! never its value. A node or the proposal server parses the section and reads no key.
//!
//! `Keys` signs and verifies session tokens: EdDSA JWTs, issuer the console's `[ui] public_url`,
//! audience `CONSOLE`, subject `local:<username>`, the role, the groups, a change-required mark
//! when set, issued at and expiry `SESSION_HOURS` later. `policy` is the password policy, one
//! pure function, over the common-password list embedded beside this file
//! (`accounts/common-passwords.txt`, its source and licence in `accounts/common-passwords.md`);
//! `hash` and `verify` are argon2id at OWASP's parameters, and `weaker` tells a stored hash to
//! redo at the next sign-in. `store` is the accounts table, its lockout counted under the row's
//! lock (`LOCKOUT_FAILURES`, `LOCKOUT_MINUTES`). `reset_admin` is `meada ui reset-admin`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Argon2, Params, Version};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use password_hash::rand_core::OsRng;
use serde::{Deserialize, Serialize};

use crate::modules::{Problem, Secret};
use crate::msg::Msg;

pub(crate) mod store;

/// The default account's username.
pub const DEFAULT_USERNAME: &str = "admin";
/// The default account's published password, which must be changed before anything else.
pub const DEFAULT_PASSWORD: &str = "admin123";
/// The variable `admin_password_env` names by default.
pub const ADMIN_PASSWORD_ENV: &str = "MEADA_ADMIN_PASSWORD";
/// The audience of a session token: the console's surface.
pub const CONSOLE: &str = "meada:console";
/// The audience of an agent token for the nodes' surface (#560), and the surface's name in a
/// request and its answer.
pub const NODES: &str = "meada:nodes";
pub const NODES_SURFACE: &str = "nodes";
/// How long a session token lives.
pub const SESSION_HOURS: u64 = 8;
/// How long an agent token lives when its request names no validity, and the most it may.
pub const AGENT_HOURS: u64 = 8;
pub const AGENT_HOURS_MAX: u64 = 168;
/// What a password holds, in Unicode scalar values (OWASP ASVS).
pub const PASSWORD_MIN: usize = 12;
pub const PASSWORD_MAX: usize = 128;
/// The most characters of a username.
pub const USERNAME_MAX: usize = 64;
/// What every local account's subject starts with, so it never equals an identity provider's.
pub const SUBJECT_PREFIX: &str = "local:";

// ponytail: the most bytes of a key file; an Ed25519 key in PEM is about 120 bytes.
const KEY_MAX: u64 = 16 << 10;
// ponytail: the most bytes of `admin_password_file`: `PASSWORD_MAX` characters of four bytes,
// with room for a newline; a longer file is refused, naming it.
const PASSWORD_FILE_MAX: u64 = 1024;
/// argon2id at OWASP's recommended parameters: 19 MiB, two iterations, one lane.
const ARGON2_MEMORY_KIB: u32 = 19 * 1024;
const ARGON2_ITERATIONS: u32 = 2;
const ARGON2_LANES: u32 = 1;
// ponytail: the costliest argon2id parameters a stored hash may carry, room to raise the
// console's own later; a row past any of them is hostile (one hash of it would take the memory
// of several) and matches nothing.
const HASH_MEMORY_MAX_KIB: u32 = 64 * 1024;
const HASH_ITERATIONS_MAX: u32 = 8;
const HASH_LANES_MAX: u32 = 4;
// ponytail: the consecutive failed password checks that lock an account, counted in a
// transaction holding its row, so parallel attempts never pass it; the lock then lasts
// `LOCKOUT_MINUTES`, and the count starts again from none.
pub const LOCKOUT_FAILURES: i32 = 10;
// ponytail: how long a lock lasts, never extended by an attempt while it holds.
pub const LOCKOUT_MINUTES: i32 = 15;
/// The characters of a password `meada ui reset-admin` generates: 32 symbols, five bits each,
/// none easily misread (no `l`, `o`, `0` or `1`).
const GENERATED: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";
/// How many characters it has: 24 of five bits, 120 bits.
const GENERATED_LENGTH: usize = 24;
/// The common-password list: lowercase, sorted by bytes, one entry per line
/// (`accounts/common-passwords.md`).
const COMMON: &str = include_str!("accounts/common-passwords.txt");

/// A local account's role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The global operator.
    Operator,
    Viewer,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Operator => "operator",
            Role::Viewer => "viewer",
        }
    }

    pub fn of(name: &str) -> Option<Self> {
        match name {
            "operator" => Some(Role::Operator),
            "viewer" => Some(Role::Viewer),
            _ => None,
        }
    }
}

/// Whether `name` is a username: 1 to `USERNAME_MAX` lowercase ASCII letters, digits, `.`, `-`
/// and `_`.
pub fn username_ok(name: &str) -> bool {
    (1..=USERNAME_MAX).contains(&name.len())
        && (name.bytes())
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// The subject of the local account `username`: `local:<username>`.
pub fn subject(username: &str) -> String {
    format!("{SUBJECT_PREFIX}{username}")
}

/// The username a local account's subject names, when it is one.
pub fn username_of(subject: &str) -> Option<&str> {
    subject
        .strip_prefix(SUBJECT_PREFIX)
        .filter(|u| username_ok(u))
}

/// Whether every one of `groups` is a name an entry's owner may be (ADR 0015).
pub fn groups_ok(groups: &[String]) -> bool {
    (groups.iter()).all(|g| meada_core::manifest::owner_problem(g).is_none())
}

/// `[accounts]` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    public_key_env: Option<String>,
    public_key_file: Option<String>,
    private_key_env: Option<String>,
    private_key_file: Option<String>,
    admin_password_env: Option<String>,
    admin_password_file: Option<String>,
    #[serde(default)]
    break_glass: bool,
    /// `[accounts.groups]`: a group to the identities it grants, as `[oidc.groups]` (#558).
    groups: Option<HashMap<String, Vec<String>>>,
    /// `[accounts.github]`: the GitHub grant provider of local accounts (#561).
    github: Option<GithubFile>,
}

/// `[accounts.github]` as written: `[oidc.github]`'s keys without `login_claim`, plus `api`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubFile {
    host: Option<String>,
    api: Option<String>,
    roles: Option<Vec<String>>,
    max_age_seconds: Option<i64>,
}

/// The GitHub REST endpoint `[accounts.github] api` defaults to on `github.com`.
pub const GITHUB_API: &str = "https://api.github.com";

impl GithubFile {
    /// The GitHub grant provider the table configures, and the REST endpoint of its host the
    /// console verifies a login on: `GITHUB_API` by default on `github.com`, required on any other
    /// host, `https` or loopback `http` as every module endpoint.
    fn validate(self) -> Result<(crate::grant::Github, String), Problem> {
        let keys = [
            "accounts.github",
            "accounts.github.host",
            "accounts.github.roles",
            "accounts.github.max_age_seconds",
        ];
        let github =
            crate::oidc::github_provider(keys, self.host, self.roles, self.max_age_seconds)?;
        let api = match (self.api, github.host.as_str()) {
            (Some(api), _) => api,
            (None, "github.com") => GITHUB_API.to_string(),
            (None, _) => {
                let missing = Msg::KeyMissing("accounts.github.api").text();
                return Err(("accounts.github", missing));
            }
        };
        if !crate::modules::reachable(&api, crate::modules::Shape::Path) {
            return Err(("accounts.github.api", Msg::OidcUrl { bare: false }.text()));
        }
        Ok((github, api.trim_end_matches('/').to_string()))
    }
}

/// The validated section: where each secret comes from, never a value.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    public_key: Option<Secret>,
    private_key: Option<Secret>,
    admin_password: Secret,
    /// `break_glass = true`: beside `[oidc]`, only the default account signs in to the console,
    /// as the global operator, and nothing else of local accounts exists (ADR 0024).
    pub break_glass: bool,
    /// `[accounts.groups]` (#558): a group of local accounts to the identities it grants, `*` for
    /// all, with `[oidc.groups]`'s rules and comparison; without it (and `[accounts.github]`)
    /// every local account reads every repository.
    pub groups: Option<HashMap<String, Vec<String>>>,
    /// `[accounts.github]` (#561), the GitHub grant provider of local accounts, which grants from
    /// a verified login: every reader outside `Manifest::read` reaches it through
    /// `Manifest::github`, which gives it its `[postgres]`.
    pub github: Option<crate::grant::Github>,
    /// `[accounts.github] api`, without a trailing slash: the one endpoint the console calls, to
    /// verify a login (`GET <api>/user`). Set exactly when `github` is.
    pub api: Option<String>,
    /// The manifest's `[postgres]`, where the accounts live: `Manifest::read` sets it, and
    /// refuses the section without it.
    pub postgres: Option<crate::postgres::Config>,
}

impl File {
    /// The section checked; each file is relative to `base`.
    pub fn validate(self, base: &Path) -> Result<Config, Problem> {
        let pair = |env: Option<String>, file: Option<String>, keys: (&'static str, &str)| {
            let both = || (keys.0, Msg::AccountsBoth(keys.1).text());
            Secret::named(env, file, base, both)
        };
        let public_key = pair(
            self.public_key_env,
            self.public_key_file,
            ("accounts.public_key_file", "public_key_env"),
        )?;
        let private_key = pair(
            self.private_key_env,
            self.private_key_file,
            ("accounts.private_key_file", "private_key_env"),
        )?;
        let admin_password = pair(
            self.admin_password_env,
            self.admin_password_file,
            ("accounts.admin_password_file", "admin_password_env"),
        )?;
        let (github, api) = match self.github.map(GithubFile::validate).transpose()? {
            Some((github, api)) => (Some(github), Some(api)),
            None => (None, None),
        };
        Ok(Config {
            public_key,
            private_key,
            admin_password: admin_password.unwrap_or(Secret::Env(ADMIN_PASSWORD_ENV.into())),
            break_glass: self.break_glass,
            groups: self.groups,
            github,
            api,
            postgres: None,
        })
    }
}

/// The dotted name of the first grant table (`[accounts.groups]`, `[accounts.github]`) the
/// manifest `table` has under an `[accounts]` with `break_glass = true`, read before the section
/// is: the break-glass account is the global operator and needs no grant, so either is refused,
/// naming it, whatever else the section says.
pub fn under_break_glass(table: &toml::Table) -> Option<&'static str> {
    let accounts = table.get("accounts").and_then(toml::Value::as_table)?;
    if accounts.get("break_glass").and_then(toml::Value::as_bool) != Some(true) {
        return None;
    }
    [("groups", "accounts.groups"), ("github", "accounts.github")]
        .into_iter()
        .find(|(name, _)| accounts.contains_key(*name))
        .map(|(_, dotted)| dotted)
}

/// Whether `token`'s `iss` claim, read without any check, is `issuer`. It routes a bearer to the
/// one verifier that may check it (under break glass, the console's own key or the identity
/// provider's), so neither ever sees the other's tokens; it admits nothing by itself.
pub fn issued_by(token: &str, issuer: &str) -> bool {
    #[derive(Deserialize)]
    struct Issued {
        iss: Option<String>,
    }
    let claims = jsonwebtoken::dangerous::insecure_decode_claims::<Issued>(token);
    claims.is_ok_and(|claims| claims.iss.as_deref() == Some(issuer))
}

/// The key of a secret of the section: `<name>_env` or `<name>_file`.
fn key_of(secret: &Secret, env: &'static str, file: &'static str) -> &'static str {
    match secret {
        Secret::Env(_) => env,
        Secret::File(_) => file,
    }
}

/// `secret`'s value, read now (at most `max` bytes for a file), or the problem naming its key and
/// never its value: an unset variable, an unreadable file.
fn read(secret: &Secret, keys: (&'static str, &'static str), max: u64) -> Result<String, Problem> {
    let key = key_of(secret, keys.0, keys.1);
    match secret.read(max) {
        Ok(Some(value)) => Ok(value),
        // Only a variable reads as nothing: a file is its text or the problem naming it.
        Ok(None) => {
            let unset = match secret {
                Secret::Env(var) => var.clone(),
                Secret::File(file) => file.display().to_string(),
            };
            Err((key, Msg::AccountsUnset(&unset).text()))
        }
        Err(problem) => Err((key, problem)),
    }
}

/// The keys of the public and the private half.
const PUBLIC: (&str, &str) = ("accounts.public_key_env", "accounts.public_key_file");
const PRIVATE: (&str, &str) = ("accounts.private_key_env", "accounts.private_key_file");

/// `secret`, the half of the pair `keys` names, or the problem that the section names none.
fn named(
    secret: &Option<Secret>,
    keys: (&'static str, &'static str),
    private: bool,
) -> Result<Secret, Problem> {
    secret.clone().ok_or_else(|| {
        let missing = Msg::AccountsKeyMissing {
            env: keys.0,
            file: keys.1,
            private,
        };
        ("accounts", missing.text())
    })
}

impl Config {
    /// The signing key's public half, read now: the start check of every surface that admits a
    /// Meada-signed token. It must be named, read and Ed25519 in PEM; a problem names its key and
    /// never the value. The private half is never read here.
    pub fn public(&self) -> Result<Public, Problem> {
        let public = named(&self.public_key, PUBLIC, false)?;
        let key = key_of(&public, PUBLIC.0, PUBLIC.1);
        let decoding = DecodingKey::from_ed_pem(read(&public, PUBLIC, KEY_MAX)?.as_bytes());
        let not_ed25519 = || (key, Msg::AccountsNotEd25519 { private: false }.text());
        decoding.map(Public).map_err(|_| not_ed25519())
    }

    /// The signing key pair, read now: the console's start check. Each half must be named, read,
    /// Ed25519 in PEM, and the private half must sign what the public half verifies; a problem
    /// names its key and never the value.
    pub fn keys(&self) -> Result<Keys, Problem> {
        named(&self.public_key, PUBLIC, false)?;
        let private = named(&self.private_key, PRIVATE, true)?;
        let private_key = key_of(&private, PRIVATE.0, PRIVATE.1);
        let public = self.public()?;
        let not_ed25519 = |key, private| (key, Msg::AccountsNotEd25519 { private }.text());
        let encoding = EncodingKey::from_ed_pem(read(&private, PRIVATE, KEY_MAX)?.as_bytes())
            .map_err(|_| not_ed25519(private_key, true))?;
        // The private half's own check happens when it signs: a probe signed now and verified
        // with the public half proves both.
        let probe = jsonwebtoken::encode(
            &Header::new(Algorithm::EdDSA),
            &Probe { probe: 1 },
            &encoding,
        )
        .map_err(|_| not_ed25519(private_key, true))?;
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.required_spec_claims.clear();
        validation.validate_exp = false;
        validation.validate_aud = false;
        if jsonwebtoken::decode::<Probe>(&probe, &public.0, &validation).is_err() {
            return Err((private_key, Msg::AccountsKeyMismatch.text()));
        }
        Ok(Keys { encoding, public })
    }

    /// What a node admits with this section and no `[oidc]` (#560): the public half read now
    /// (`public`), and the console's `[ui] public_url`, `issuer`, every token's issuer.
    pub fn nodes(&self, issuer: &str) -> Result<Nodes, Problem> {
        let public = self.public()?;
        let issuer = issuer.trim_end_matches('/').to_string();
        Ok(Nodes { public, issuer })
    }

    /// The default account's initial password the deployer set: `None` when the variable is
    /// unset or empty (the published default applies), the problem naming the key when the file
    /// cannot be read. Read only when the default account is created.
    pub fn admin_password(&self) -> Result<Option<String>, Problem> {
        match &self.admin_password {
            // A value that is not UTF-8 is refused, never taken for an unset variable.
            Secret::Env(var) => match std::env::var(var) {
                Ok(value) => Ok(Some(value).filter(|v| !v.is_empty())),
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(std::env::VarError::NotUnicode(_)) => {
                    Err((ADMIN_PASSWORD.0, Msg::AccountsNotUtf8.text()))
                }
            },
            file => read(file, ADMIN_PASSWORD, PASSWORD_FILE_MAX).map(Some),
        }
    }

    /// The key that names the initial password, for a policy refusal.
    pub fn admin_password_key(&self) -> &'static str {
        key_of(&self.admin_password, ADMIN_PASSWORD.0, ADMIN_PASSWORD.1)
    }
}

/// The keys of the initial password: the variable's and the file's.
const ADMIN_PASSWORD: (&str, &str) = (
    "accounts.admin_password_env",
    "accounts.admin_password_file",
);

#[derive(Serialize, Deserialize)]
struct Probe {
    probe: u8,
}

/// The claims of a session token, and nothing else.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    /// `local:<username>`.
    pub sub: String,
    pub role: Role,
    pub groups: Vec<String>,
    /// The account's verified GitHub login (#561), lowercase, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    /// Set only while the password must be changed: the token then opens the change alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub change_required: bool,
    /// An agent token's random id, which its audit record names (#560); a session token has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    pub iat: u64,
    pub exp: u64,
}

impl Claims {
    /// The username the subject names.
    pub fn username(&self) -> &str {
        username_of(&self.sub).unwrap_or_default()
    }
}

/// Whom a token is signed for: an account's username, its role, its groups and its verified
/// login, as its row (or break glass) stands when the token is issued.
pub struct Holder<'a> {
    pub username: &'a str,
    pub role: Role,
    pub groups: &'a [String],
    pub login: Option<&'a str>,
}

impl Holder<'_> {
    /// The claims of a token for this holder from `issuer` for `audience`, issued now and valid
    /// `hours`.
    fn claims(
        &self,
        issuer: &str,
        audience: &str,
        hours: u64,
        change_required: bool,
        jti: Option<String>,
    ) -> Claims {
        let iat = crate::modules::now();
        Claims {
            iss: issuer.to_string(),
            aud: audience.to_string(),
            sub: subject(self.username),
            role: self.role,
            groups: self.groups.to_vec(),
            login: self.login.map(str::to_string),
            change_required,
            jti,
            iat,
            exp: iat + hours * 3600,
        }
    }
}

/// The signing key pair, the console's alone.
pub struct Keys {
    encoding: EncodingKey,
    public: Public,
}

/// The signing key's public half, which every surface verifies a Meada-signed token with.
pub struct Public(DecodingKey);

/// A node's admission of agent tokens for the nodes (#560): the public half and the console's
/// issuer.
pub struct Nodes {
    public: Public,
    issuer: String,
}

impl Nodes {
    /// The claims of `headers`' bearer when it is an agent token for the nodes of this console
    /// (`Public::verify` with `NODES`), never one awaiting a password change; else whether a
    /// bearer was sent, for the challenge of the 401.
    pub fn admit(&self, headers: &hyper::HeaderMap) -> Result<Claims, bool> {
        match crate::oidc::bearer(headers) {
            Ok(Some(token)) => (self.public.verify(token, &self.issuer, NODES))
                .filter(|claims| !claims.change_required)
                .ok_or(true),
            Ok(None) => Err(false),
            Err(_) => Err(true),
        }
    }
}

impl Keys {
    /// The public key's fingerprint (#558), which the admission view shows so an operator can tell
    /// which key pair the console holds: `blake3:` and the hex BLAKE3 hash of the raw 32-byte
    /// Ed25519 public key (the last 32 bytes of its DER). Never a secret.
    pub fn fingerprint(&self) -> String {
        let raw = (self.public.0.try_get_as_bytes())
            .expect("an Ed25519 key read from PEM holds its raw bytes");
        format!("blake3:{}", blake3::hash(raw).to_hex())
    }

    /// A session token for `holder` from `issuer`, now: the token and its expiry in seconds
    /// since the epoch.
    pub fn session(
        &self,
        issuer: &str,
        holder: &Holder,
        change_required: bool,
    ) -> Result<(String, u64), String> {
        let claims = holder.claims(issuer, CONSOLE, SESSION_HOURS, change_required, None);
        self.signed(&claims)
    }

    /// An agent token for `holder` from `issuer` for the surface `audience`, valid `hours` from
    /// now (#560): the session token's claims with that audience, a random token id and never a
    /// change-required mark. The token, its expiry in seconds since the epoch and its id.
    pub fn agent(
        &self,
        issuer: &str,
        holder: &Holder,
        audience: &str,
        hours: u64,
    ) -> Result<(String, u64, String), String> {
        use password_hash::rand_core::RngCore;
        let mut id = [0u8; 16];
        OsRng.fill_bytes(&mut id);
        let id: String = id.iter().map(|b| format!("{b:02x}")).collect();
        let claims = holder.claims(issuer, audience, hours, false, Some(id.clone()));
        let (token, exp) = self.signed(&claims)?;
        Ok((token, exp, id))
    }

    /// `claims` signed with EdDSA, and their expiry.
    fn signed(&self, claims: &Claims) -> Result<(String, u64), String> {
        let header = Header::new(Algorithm::EdDSA);
        let token = jsonwebtoken::encode(&header, claims, &self.encoding);
        Ok((token.map_err(|e| e.to_string())?, claims.exp))
    }

    /// `Public::verify` with this pair's public half.
    pub fn verify(&self, token: &str, issuer: &str, audience: &str) -> Option<Claims> {
        self.public.verify(token, issuer, audience)
    }
}

impl Public {
    /// The claims of `token` when it passes: EdDSA only, no `crit`, signed by this key's pair,
    /// issued by `issuer` for `audience`, `exp` and `nbf` with the identity provider's leeway, a
    /// subject `local:<username>`, a role, groups that are owner names, and a verified login that
    /// is a login when there is one (#561). Nothing says why a token fails: every failure is the
    /// same 401.
    pub fn verify(&self, token: &str, issuer: &str, audience: &str) -> Option<Claims> {
        let header = jsonwebtoken::decode_header(token).ok()?;
        if header.alg != Algorithm::EdDSA || header.crit.is_some() {
            return None;
        }
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.leeway = crate::oidc::LEEWAY;
        validation.validate_nbf = true;
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let claims = jsonwebtoken::decode::<Claims>(token, &self.0, &validation).ok()?;
        let claims = claims.claims;
        let login_ok = claims.login.as_deref().is_none_or(crate::grant::login_ok);
        (username_of(&claims.sub).is_some() && groups_ok(&claims.groups) && login_ok)
            .then_some(claims)
    }
}

/// A password the policy refuses, and why: the first rule it fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Fewer than `PASSWORD_MIN` characters.
    Short,
    /// More than `PASSWORD_MAX` characters.
    Long,
    /// The published default password.
    Default,
    /// The username, ignoring case.
    Username,
    /// An entry of the common-password list, ignoring case.
    Common,
    /// The current password, in a change: not `policy`'s, which has no current password.
    Current,
}

impl Rule {
    /// The rule's name in an API error.
    pub fn code(self) -> &'static str {
        match self {
            Rule::Short => "too_short",
            Rule::Long => "too_long",
            Rule::Default => "default_password",
            Rule::Username => "username",
            Rule::Common => "common_password",
            Rule::Current => "current_password",
        }
    }

    /// The message naming the rule, never the password.
    pub fn text(self) -> String {
        match self {
            Rule::Short => Msg::AccountsPasswordShort(PASSWORD_MIN).text(),
            Rule::Long => Msg::AccountsPasswordLong(PASSWORD_MAX).text(),
            Rule::Default => Msg::AccountsPasswordDefault.text(),
            Rule::Username => Msg::AccountsPasswordUsername.text(),
            Rule::Common => Msg::AccountsPasswordCommon.text(),
            Rule::Current => Msg::AccountsPasswordCurrent.text(),
        }
    }
}

/// The password policy (OWASP ASVS), shared by every route and the CLI: 12 to 128 characters
/// counted as Unicode scalar values, any character, no composition rule and no truncation;
/// refused when it is the default password, the username ignoring case, or an entry of the
/// common-password list ignoring case. The first rule it fails, in that order.
pub fn policy(password: &str, username: &str) -> Result<(), Rule> {
    let length = password.chars().count();
    let lower = password.to_lowercase();
    if length < PASSWORD_MIN {
        Err(Rule::Short)
    } else if length > PASSWORD_MAX {
        Err(Rule::Long)
    } else if password == DEFAULT_PASSWORD {
        Err(Rule::Default)
    } else if lower == username.to_lowercase() {
        Err(Rule::Username)
    } else if common().binary_search(&lower.as_str()).is_ok() {
        Err(Rule::Common)
    } else {
        Ok(())
    }
}

/// The entries of the common-password list, split once.
fn common() -> &'static [&'static str] {
    static LIST: OnceLock<Vec<&'static str>> = OnceLock::new();
    LIST.get_or_init(|| COMMON.lines().collect())
}

/// A password for `meada ui reset-admin`: `GENERATED_LENGTH` characters of `GENERATED` from the
/// operating system's generator, one the policy takes.
pub fn generated() -> String {
    use password_hash::rand_core::RngCore;
    loop {
        let mut bytes = [0u8; GENERATED_LENGTH];
        OsRng.fill_bytes(&mut bytes);
        // 256 is a multiple of 32: every symbol is as likely as every other.
        let password: String = (bytes.iter())
            .map(|b| GENERATED[usize::from(*b) % GENERATED.len()] as char)
            .collect();
        if policy(&password, DEFAULT_USERNAME).is_ok() {
            return password;
        }
    }
}

/// argon2id at OWASP's recommended parameters.
fn argon2() -> Argon2<'static> {
    let params = Params::new(ARGON2_MEMORY_KIB, ARGON2_ITERATIONS, ARGON2_LANES, None)
        .expect("valid argon2 parameters");
    Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params)
}

/// The PHC string of `password` under argon2id, with a fresh salt: the parameters ride in it, so
/// they can be raised later.
pub fn hash(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    let hashed = argon2().hash_password(password.as_bytes(), &salt);
    hashed.map(|h| h.to_string()).map_err(|e| e.to_string())
}

/// Whether `password` is the one `phc`, an argon2id PHC string, hashes, with `phc`'s own
/// parameters. A string that is no stored hash (`is_hash`) matches nothing.
pub fn verify(password: &str, phc: &str) -> bool {
    stored(phc).is_some_and(|parsed| {
        argon2()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

/// Whether `phc` is what a stored hash must be: an argon2id PHC string whose parameters cost no
/// more than `HASH_MEMORY_MAX_KIB`, `HASH_ITERATIONS_MAX` and `HASH_LANES_MAX`.
pub fn is_hash(phc: &str) -> bool {
    stored(phc).is_some()
}

/// `phc` parsed when it is a stored hash (`is_hash`).
fn stored(phc: &str) -> Option<PasswordHash<'_>> {
    let parsed = PasswordHash::new(phc).ok()?;
    let params = Params::try_from(&parsed).ok()?;
    let bounded = params.m_cost() <= HASH_MEMORY_MAX_KIB
        && params.t_cost() <= HASH_ITERATIONS_MAX
        && params.p_cost() <= HASH_LANES_MAX;
    (parsed.algorithm.as_str() == "argon2id" && bounded).then_some(parsed)
}

/// Whether the stored hash `phc` is weaker than the console's own parameters (less memory, fewer
/// iterations, or an older argon2 version), so a sign-in that proves its password redoes it.
pub fn weaker(phc: &str) -> bool {
    stored(phc).is_some_and(|parsed| {
        let params = Params::try_from(&parsed).ok();
        let version = parsed.version.and_then(|v| Version::try_from(v).ok());
        params.is_none_or(|p| p.m_cost() < ARGON2_MEMORY_KIB || p.t_cost() < ARGON2_ITERATIONS)
            || version != Some(Version::V0x13)
    })
}

/// A fixed hash the sign-in checks an unknown username against, so that every refusal costs one
/// hash. Computed once, at the console's start.
pub fn dummy() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| hash("meada: no such account").expect("a hash"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(toml: &str) -> Result<Config, Problem> {
        let file: File = toml::from_str(toml).unwrap();
        file.validate(Path::new("/etc/meada"))
    }

    #[test]
    fn the_section_names_its_secrets_and_takes_one_of_each_pair() {
        let config = section("").unwrap();
        assert_eq!(config.public_key, None);
        assert_eq!(config.private_key, None);
        assert_eq!(
            config.admin_password,
            Secret::Env(ADMIN_PASSWORD_ENV.into())
        );
        let config = section(
            "public_key_file = \"keys/public.pem\"\nprivate_key_env = \"KEY\"\n\
             admin_password_file = \"admin\"",
        )
        .unwrap();
        assert_eq!(
            config.public_key,
            Some(Secret::File("/etc/meada/keys/public.pem".into()))
        );
        assert_eq!(config.private_key, Some(Secret::Env("KEY".into())));
        assert_eq!(
            config.admin_password,
            Secret::File("/etc/meada/admin".into())
        );
        for (toml, key) in [
            (
                "public_key_env = \"A\"\npublic_key_file = \"b\"",
                "accounts.public_key_file",
            ),
            (
                "private_key_env = \"A\"\nprivate_key_file = \"b\"",
                "accounts.private_key_file",
            ),
            (
                "admin_password_env = \"A\"\nadmin_password_file = \"b\"",
                "accounts.admin_password_file",
            ),
        ] {
            let (found, message) = section(toml).unwrap_err();
            assert_eq!(found, key);
            assert!(message.contains("_env"), "{message}");
        }
    }

    /// `[accounts.github]` (#561): `[oidc.github]`'s keys without `login_claim`, plus `api`,
    /// GitHub's by default on `github.com` and required on another host, `https` or loopback
    /// `http`; each bad key named with this table's prefix.
    #[test]
    fn the_github_table_defaults_its_api_on_github_com_and_names_each_bad_key() {
        let table = |rest: &str| format!("[github]\n{rest}");
        let config = section(&table("max_age_seconds = 5400\n")).unwrap();
        let github = config.github.unwrap();
        assert_eq!(github.host, "github.com");
        assert_eq!(github.roles, crate::oidc::ROLES);
        assert_eq!(github.max_age, std::time::Duration::from_secs(5400));
        assert_eq!(config.api.as_deref(), Some(GITHUB_API));
        let custom = "host = \"ghe.example\"\napi = \"https://ghe.example/api/v3/\"\n\
                      roles = [\"read\"]\nmax_age_seconds = 60\n";
        let config = section(&table(custom)).unwrap();
        assert_eq!(config.github.unwrap().host, "ghe.example");
        assert_eq!(config.api.as_deref(), Some("https://ghe.example/api/v3"));
        let loopback = "api = \"http://127.0.0.1:9/\"\nmax_age_seconds = 60\n";
        let config = section(&table(loopback)).unwrap();
        assert_eq!(config.api.as_deref(), Some("http://127.0.0.1:9"));
        assert!(section("").unwrap().github.is_none());
        assert!(section("").unwrap().api.is_none());
        for (rest, key) in [
            ("", "accounts.github"),
            ("max_age_seconds = 59\n", "accounts.github.max_age_seconds"),
            (
                "host = \"ghe.example\"\nmax_age_seconds = 60\n",
                "accounts.github",
            ),
            (
                "host = \"GHE.example\"\nmax_age_seconds = 60\n",
                "accounts.github.host",
            ),
            (
                "roles = []\nmax_age_seconds = 60\n",
                "accounts.github.roles",
            ),
            (
                "api = \"http://api.github.com\"\nmax_age_seconds = 60\n",
                "accounts.github.api",
            ),
            (
                "api = \"https://api.github.com?x=1\"\nmax_age_seconds = 60\n",
                "accounts.github.api",
            ),
            (
                "api = \"https://u@api.github.com\"\nmax_age_seconds = 60\n",
                "accounts.github.api",
            ),
        ] {
            assert_eq!(
                section(&table(rest)).err().map(|e| e.0),
                Some(key),
                "{rest}"
            );
        }
        // `login_claim` belongs to an identity provider's token, which local accounts have not.
        let claim = "login_claim = \"login\"\nmax_age_seconds = 60\n";
        assert!(toml::from_str::<File>(&table(claim)).is_err());
    }

    /// `break_glass` is off unless set; set, it refuses the grant tables, named before the section
    /// is read; without it they are left to the section (#563).
    #[test]
    fn break_glass_is_off_by_default_and_refuses_the_grant_tables() {
        assert!(!section("").unwrap().break_glass);
        assert!(section("break_glass = true").unwrap().break_glass);
        assert!(!section("break_glass = false").unwrap().break_glass);
        let table = |toml: &str| toml::from_str::<toml::Table>(toml).unwrap();
        for (toml, expected) in [
            (
                "[accounts]\nbreak_glass = true\n[accounts.groups]\nops = [\"*\"]\n",
                Some("accounts.groups"),
            ),
            (
                "[accounts]\nbreak_glass = true\n[accounts.github]\nhost = \"github.com\"\n",
                Some("accounts.github"),
            ),
            (
                "[accounts]\nbreak_glass = true\n[accounts.groups]\n[accounts.github]\n",
                Some("accounts.groups"),
            ),
            ("[accounts]\nbreak_glass = true\n", None),
            ("[accounts]\n[accounts.groups]\nops = [\"*\"]\n", None),
            ("[accounts]\nbreak_glass = false\n[accounts.github]\n", None),
            // Not a boolean: the section's own parse names it.
            (
                "[accounts]\nbreak_glass = \"yes\"\n[accounts.groups]\n",
                None,
            ),
            ("accounts = 1\n", None),
            ("", None),
        ] {
            assert_eq!(under_break_glass(&table(toml)), expected, "{toml}");
        }
    }

    #[test]
    fn the_policy_names_the_first_rule_a_password_fails() {
        let long = "x".repeat(PASSWORD_MAX + 1);
        let longest = "é".repeat(PASSWORD_MAX);
        for (password, username, expected) in [
            ("", "alice", Err(Rule::Short)),
            ("elevenchars", "alice", Err(Rule::Short)),
            // Characters, not bytes: eleven two-byte characters are still short.
            ("ééééééééééé", "alice", Err(Rule::Short)),
            // A common password too short to reach the list: the length rule names it.
            ("password", "alice", Err(Rule::Short)),
            (long.as_str(), "alice", Err(Rule::Long)),
            (longest.as_str(), "alice", Ok(())),
            ("twelve chars", "alice", Ok(())),
            ("correct horse battery", "alice", Ok(())),
            ("ALICE.SMITH-1", "alice.smith-1", Err(Rule::Username)),
            ("alice.smith-1", "alice.smith-1", Err(Rule::Username)),
            ("alice.smith-12", "alice.smith-1", Ok(())),
            // The username comes before the list: a common username is named as the username.
            ("password1234", "password1234", Err(Rule::Username)),
            // The list's entries, ignoring case, and its first and last.
            ("password1234", "alice", Err(Rule::Common)),
            ("PassWord1234", "alice", Err(Rule::Common)),
            ("QWERTYUIOPASDFGHJKL", "alice", Err(Rule::Common)),
            ("123456789012", "alice", Err(Rule::Common)),
            (common()[0], "alice", Err(Rule::Common)),
            (common()[common().len() - 1], "alice", Err(Rule::Common)),
            // Near an entry is not an entry.
            ("password12345x", "alice", Ok(())),
            ("password1234 ", "alice", Ok(())),
        ] {
            assert_eq!(policy(password, username), expected, "{password}");
        }
        // The default password is shorter than the minimum: the length rule names it first, and
        // the default rule stands for a longer default or a shorter minimum.
        assert!(DEFAULT_PASSWORD.chars().count() < PASSWORD_MIN);
        assert_eq!(policy(DEFAULT_PASSWORD, "admin"), Err(Rule::Short));
        let rules = [
            Rule::Short,
            Rule::Long,
            Rule::Default,
            Rule::Username,
            Rule::Common,
            Rule::Current,
        ];
        let codes: std::collections::BTreeSet<&str> = rules.iter().map(|r| r.code()).collect();
        assert_eq!(codes.len(), rules.len());
        for rule in rules {
            assert!(!rule.text().is_empty() && !rule.code().is_empty());
        }
        assert!(!Rule::Common.text().contains("password1234"));
    }

    /// The embedded list is what `common-passwords.md` says: sorted by bytes without repeats,
    /// lowercase, each entry 12 to 128 characters with no whitespace at either end, so a
    /// regenerated list that breaks the filter fails here.
    #[test]
    fn the_common_password_list_is_sorted_lowercase_and_past_the_length_rule() {
        let list = common();
        assert_eq!(list.len(), 46146);
        assert!(COMMON.ends_with('\n') && !COMMON.contains('\r'));
        assert!(list.windows(2).all(|w| w[0] < w[1]), "sorted, no repeats");
        for entry in list {
            assert_eq!(entry.to_lowercase(), *entry, "{entry}");
            assert!(
                (PASSWORD_MIN..=PASSWORD_MAX).contains(&entry.chars().count()),
                "{entry}"
            );
            assert_eq!(entry.trim(), *entry, "{entry}");
            assert!(!entry.chars().any(char::is_control), "{entry}");
        }
    }

    #[test]
    fn a_generated_password_passes_the_policy_and_is_never_the_same() {
        let first = generated();
        assert_eq!(first.chars().count(), GENERATED_LENGTH);
        assert!(first.bytes().all(|b| GENERATED.contains(&b)), "{first}");
        assert_eq!(policy(&first, DEFAULT_USERNAME), Ok(()));
        assert_ne!(generated(), first);
    }

    #[test]
    fn a_username_is_lowercase_ascii_letters_digits_dots_dashes_and_underscores() {
        for name in ["admin", "a", "alice.smith-1_x", &"a".repeat(USERNAME_MAX)] {
            assert!(username_ok(name), "{name}");
        }
        for name in [
            "",
            "Admin",
            "al ice",
            "alice@x",
            "ali\u{e9}",
            &"a".repeat(USERNAME_MAX + 1),
        ] {
            assert!(!username_ok(name), "{name}");
        }
    }

    fn holder<'a>(
        username: &'a str,
        role: Role,
        groups: &'a [String],
        login: Option<&'a str>,
    ) -> Holder<'a> {
        Holder {
            username,
            role,
            groups,
            login,
        }
    }

    /// A fresh Ed25519 key pair in PEM: the private half (PKCS #8) and the public half (SPKI).
    pub(crate) fn pair() -> (String, String) {
        use ring::signature::KeyPair;
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let mut spki = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        spki.extend_from_slice(pair.public_key().as_ref());
        (pem("PRIVATE KEY", pkcs8.as_ref()), pem("PUBLIC KEY", &spki))
    }

    fn pem(label: &str, der: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut text = String::new();
        for chunk in der.chunks(3) {
            let n =
                chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
            for i in 0..4 {
                let c = match i <= chunk.len() {
                    true => ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char,
                    false => '=',
                };
                text.push(c);
            }
        }
        format!("-----BEGIN {label}-----\n{text}\n-----END {label}-----\n")
    }

    /// The fingerprint is the BLAKE3 hash of the raw public key, the last 32 bytes of its DER,
    /// as `openssl pkey -pubin -outform DER | tail -c 32 | b3sum` computes it.
    #[test]
    fn the_fingerprint_hashes_the_raw_public_key() {
        let (private, public) = pair();
        let keys = keyed("fingerprint", &public, &private).keys().unwrap();
        let body: String = (public.lines())
            .filter(|l| !l.starts_with("-----"))
            .collect();
        let der = decoded(&body);
        let raw = &der[der.len() - 32..];
        assert_eq!(
            keys.fingerprint(),
            format!("blake3:{}", blake3::hash(raw).to_hex())
        );
        let (other_private, other_public) = pair();
        let other = keyed("fingerprint2", &other_public, &other_private);
        assert_ne!(other.keys().unwrap().fingerprint(), keys.fingerprint());
    }

    /// `text`, standard base64 with padding, decoded.
    fn decoded(text: &str) -> Vec<u8> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let sextets: Vec<u32> = (text.bytes().filter(|b| *b != b'='))
            .map(|b| ALPHABET.iter().position(|a| *a == b).unwrap() as u32)
            .collect();
        let mut bytes = Vec::new();
        for chunk in sextets.chunks(4) {
            let n = (chunk.iter().enumerate()).fold(0u32, |n, (i, s)| n | s << (18 - 6 * i));
            bytes.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
        }
        bytes
    }

    /// `[accounts]` over the key files `public` and `private` written under a directory of `case`.
    fn keyed(case: &str, public: &str, private: &str) -> Config {
        let dir =
            std::env::temp_dir().join(format!("meada-accounts-{case}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("public.pem"), public).unwrap();
        std::fs::write(dir.join("private.pem"), private).unwrap();
        let file: File =
            toml::from_str("public_key_file = \"public.pem\"\nprivate_key_file = \"private.pem\"")
                .unwrap();
        file.validate(&dir).unwrap()
    }

    #[test]
    fn the_key_pair_must_be_named_read_ed25519_and_matched() {
        let (private, public) = pair();
        let (other_private, other_public) = pair();
        assert!(keyed("ok", &public, &private).keys().is_ok());
        let refused = |config: Config| config.keys().err().unwrap();
        let (key, message) = refused(keyed("mismatch", &other_public, &private));
        assert_eq!(key, "accounts.private_key_file");
        assert!(message.contains("does not match"), "{message}");
        let (key, message) = refused(keyed("swapped", &private, &other_private));
        assert_eq!(key, "accounts.public_key_file");
        assert!(message.contains("Ed25519 public key"), "{message}");
        let (key, _) = refused(keyed("garbage", &public, "not a key"));
        assert_eq!(key, "accounts.private_key_file");
        // No value reaches a message.
        for (_, message) in [
            refused(keyed("leak1", &other_public, &private)),
            refused(keyed("leak2", &public, "not a key")),
        ] {
            assert!(
                !message.contains("BEGIN") && !message.contains("not a key"),
                "{message}"
            );
        }
        let missing = section("public_key_env = \"MEADA_TEST_NO_SUCH_KEY_7c1e\"");
        let (key, message) = refused(missing.unwrap());
        assert_eq!(key, "accounts");
        assert!(message.contains("private_key_env"), "{message}");
        let unset = section(
            "public_key_env = \"MEADA_TEST_NO_SUCH_KEY_7c1e\"\nprivate_key_env = \"MEADA_TEST_NO_SUCH_KEY_7c1f\"",
        );
        let (key, message) = refused(unset.unwrap());
        assert_eq!(key, "accounts.public_key_env");
        assert!(message.contains("MEADA_TEST_NO_SUCH_KEY_7c1e"), "{message}");
        let unreadable = section("public_key_file = \"no-such.pem\"\nprivate_key_env = \"K\"");
        assert_eq!(refused(unreadable.unwrap()).0, "accounts.public_key_file");
    }

    #[test]
    fn a_key_file_or_a_password_file_past_its_limit_is_refused_naming_the_file() {
        let (private, _) = pair();
        let padded = format!("{private}{}", "\n".repeat(KEY_MAX as usize));
        let (key, message) = keyed("big-key", &padded, &private).keys().err().unwrap();
        assert_eq!(key, "accounts.public_key_file");
        assert!(
            message.contains(&format!("larger than {KEY_MAX} bytes")),
            "{message}"
        );
        let dir = std::env::temp_dir().join(format!("meada-accounts-pw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("admin"),
            "p".repeat(PASSWORD_FILE_MAX as usize + 1),
        )
        .unwrap();
        let file: File = toml::from_str("admin_password_file = \"admin\"").unwrap();
        let config = file.validate(&dir).unwrap();
        let (key, message) = config.admin_password().unwrap_err();
        assert_eq!(key, "accounts.admin_password_file");
        assert!(
            message.contains("larger than 1024 bytes") && !message.contains("ppp"),
            "{message}"
        );
        std::fs::write(dir.join("admin"), "a chosen password\n").unwrap();
        assert_eq!(
            config.admin_password().unwrap().as_deref(),
            Some("a chosen password")
        );
    }

    #[test]
    fn a_session_token_passes_only_for_its_issuer_audience_key_and_lifetime() {
        let (private, public) = pair();
        let keys = keyed("tokens", &public, &private).keys().unwrap();
        let issuer = "https://console.example";
        let groups = ["team-web".to_string()];
        let (token, exp) = keys
            .session(issuer, &holder("alice", Role::Viewer, &groups, None), true)
            .unwrap();
        let claims = keys.verify(&token, issuer, CONSOLE).unwrap();
        assert_eq!(
            (claims.sub.as_str(), claims.username()),
            ("local:alice", "alice")
        );
        assert_eq!(
            (
                claims.role,
                claims.groups.as_slice(),
                claims.change_required
            ),
            (Role::Viewer, &groups[..], true)
        );
        assert_eq!(claims.exp, exp);
        assert_eq!(exp - claims.iat, SESSION_HOURS * 3600);
        assert!(
            keys.verify(&token, "https://other.example", CONSOLE)
                .is_none()
        );
        assert!(keys.verify(&token, issuer, "meada:nodes").is_none());
        let (other_private, other_public) = pair();
        let other = keyed("tokens-other", &other_public, &other_private)
            .keys()
            .unwrap();
        assert!(other.verify(&token, issuer, CONSOLE).is_none());
        // Without the mark, the claim is absent.
        let (plain, _) = keys
            .session(issuer, &holder("alice", Role::Operator, &[], None), false)
            .unwrap();
        let payload = plain.split('.').nth(1).unwrap();
        assert!(!payload.is_empty());
        assert!(
            !keys
                .verify(&plain, issuer, CONSOLE)
                .unwrap()
                .change_required
        );
        // Expired, or not yet valid, past the leeway; HS256 and a subject that is no local one.
        let sign = |claims: serde_json::Value| {
            jsonwebtoken::encode(&Header::new(Algorithm::EdDSA), &claims, &keys.encoding).unwrap()
        };
        let now = crate::modules::now();
        let base = serde_json::json!({
            "iss": issuer, "aud": CONSOLE, "sub": "local:alice", "role": "viewer",
            "groups": [], "iat": now, "exp": now + 60,
        });
        assert!(keys.verify(&sign(base.clone()), issuer, CONSOLE).is_some());
        let mut with_login = base.clone();
        with_login["login"] = serde_json::json!("ana-1");
        let claims = keys.verify(&sign(with_login), issuer, CONSOLE).unwrap();
        assert_eq!(claims.login.as_deref(), Some("ana-1"));
        for edit in [
            ("exp", serde_json::json!(now - 2 * crate::oidc::LEEWAY)),
            ("nbf", serde_json::json!(now + 2 * crate::oidc::LEEWAY)),
            ("sub", serde_json::json!("alice")),
            ("sub", serde_json::json!("local:Alice")),
            ("role", serde_json::json!("root")),
            // A login claim that is no stored login (#561): uppercase, a bad character, a number.
            ("login", serde_json::json!("Ana")),
            ("login", serde_json::json!("ana smith")),
            ("login", serde_json::json!(7)),
        ] {
            let mut claims = base.clone();
            claims[edit.0] = edit.1.clone();
            assert!(
                keys.verify(&sign(claims), issuer, CONSOLE).is_none(),
                "{edit:?}"
            );
        }
        let mut no_exp = base.clone();
        no_exp.as_object_mut().unwrap().remove("exp");
        assert!(keys.verify(&sign(no_exp), issuer, CONSOLE).is_none());
        let hs256 = jsonwebtoken::encode(
            &Header::new(Algorithm::HS256),
            &base,
            &EncodingKey::from_secret(public.as_bytes()),
        )
        .unwrap();
        assert!(keys.verify(&hs256, issuer, CONSOLE).is_none());
        // A header naming critical extensions is refused, whatever it names.
        let mut critical = Header::new(Algorithm::EdDSA);
        critical.crit = Some(vec!["exp".into()]);
        let crit = jsonwebtoken::encode(&critical, &base, &keys.encoding).unwrap();
        assert!(keys.verify(&crit, issuer, CONSOLE).is_none());
    }

    /// An agent token (#560) carries the session token's claims with its surface's audience, a
    /// random id and no change-required mark, lives `hours`, and a node admits it with the public
    /// half alone (the private file gone), the issuer's trailing slash aside; the console's
    /// audience refuses it, and a node refuses a session token, no bearer and another scheme.
    #[test]
    fn an_agent_token_is_for_its_surface_alone_with_a_random_id() {
        let (private, public) = pair();
        let config = keyed("agent", &public, &private);
        let keys = config.keys().unwrap();
        let issuer = "https://console.example";
        let groups = ["team-web".to_string()];
        let (token, exp, id) = (keys.agent(
            issuer,
            &holder("alice", Role::Viewer, &groups, Some("ana")),
            NODES,
            24,
        ))
        .unwrap();
        let dir = std::env::temp_dir().join(format!("meada-accounts-agent-{}", std::process::id()));
        std::fs::remove_file(dir.join("private.pem")).unwrap();
        let nodes = config.nodes(&format!("{issuer}/")).unwrap();
        let mut headers = hyper::HeaderMap::new();
        let mut send = |value: &str| {
            let value = hyper::header::HeaderValue::from_str(value).unwrap();
            headers.insert(hyper::header::AUTHORIZATION, value);
            nodes.admit(&headers)
        };
        let claims = send(&format!("Bearer {token}")).unwrap();
        assert_eq!(
            (claims.sub.as_str(), claims.role, claims.groups.as_slice()),
            ("local:alice", Role::Viewer, &groups[..])
        );
        assert_eq!(claims.jti.as_deref(), Some(id.as_str()));
        // The verified login rides in the token (#561).
        assert_eq!(claims.login.as_deref(), Some("ana"));
        assert!(!claims.change_required);
        assert_eq!((claims.exp, exp - claims.iat), (exp, 24 * 3600));
        assert!(
            id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "{id}"
        );
        let again = keys.agent(
            issuer,
            &holder("alice", Role::Viewer, &groups, None),
            NODES,
            1,
        );
        assert_ne!(again.unwrap().2, id);
        assert!(keys.verify(&token, issuer, CONSOLE).is_none());
        let (session, _) =
            (keys.session(issuer, &holder("alice", Role::Viewer, &groups, None), false)).unwrap();
        assert_eq!(send(&format!("Bearer {session}")).err(), Some(true));
        assert_eq!(send("Bearer ").err(), Some(true));
        assert_eq!(send(&format!("Basic {token}")).err(), Some(false));
        // The public half alone: a node never needs the private one.
        assert!(config.keys().is_err());
    }

    /// A bearer is routed by its issuer read without a check: this console's session token by
    /// its own issuer, never another's; what is no JWT, or has no issuer, by none.
    #[test]
    fn a_token_is_routed_by_its_unchecked_issuer() {
        let (private, public) = pair();
        let keys = keyed("issued", &public, &private).keys().unwrap();
        let issuer = "https://console.example";
        let (token, _) = keys
            .session(issuer, &holder("admin", Role::Operator, &[], None), false)
            .unwrap();
        assert!(issued_by(&token, issuer));
        assert!(!issued_by(&token, "https://console.example/"));
        assert!(!issued_by(&token, "https://id.example"));
        let header = Header::new(Algorithm::EdDSA);
        let unsigned =
            jsonwebtoken::encode(&header, &serde_json::json!({"sub": "x"}), &keys.encoding);
        assert!(!issued_by(&unsigned.unwrap(), issuer));
        let numeric = jsonwebtoken::encode(&header, &serde_json::json!({"iss": 1}), &keys.encoding);
        assert!(!issued_by(&numeric.unwrap(), issuer));
        for bad in ["", "a.b", "not a token", "a.b.c"] {
            assert!(!issued_by(bad, issuer), "{bad}");
        }
    }

    #[test]
    fn a_hash_is_argon2id_at_owasp_parameters_and_verifies_only_its_password() {
        let phc = hash("correct horse battery").unwrap();
        assert!(phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{phc}");
        assert!(is_hash(&phc));
        assert!(verify("correct horse battery", &phc));
        assert!(!verify("correct horse batterY", &phc));
        // A fresh salt per hash.
        assert_ne!(hash("correct horse battery").unwrap(), phc);
        for bad in [
            "",
            "admin123",
            "$argon2i$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaA",
        ] {
            assert!(!is_hash(bad) && !verify("admin123", bad), "{bad}");
        }
        assert!(is_hash(dummy()));
        // A hostile row's parameters are bounded: one past a ceiling is no stored hash and is
        // never computed; the iteration and lane ceilings themselves pass.
        let salt = SaltString::generate(&mut OsRng);
        let with = |m: u32, t: u32, p: u32| {
            let params = Params::new(m, t, p, None).unwrap();
            let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);
            let phc = argon2
                .hash_password(b"correct horse battery", &salt)
                .unwrap();
            phc.to_string()
        };
        let ceiling = with(
            HASH_MEMORY_MAX_KIB / 64,
            HASH_ITERATIONS_MAX,
            HASH_LANES_MAX,
        );
        assert!(is_hash(&ceiling) && verify("correct horse battery", &ceiling));
        let costly = with(HASH_MEMORY_MAX_KIB / 64, 1, 1);
        for (from, to) in [
            (
                format!("m={}", HASH_MEMORY_MAX_KIB / 64),
                format!("m={}", HASH_MEMORY_MAX_KIB + 1),
            ),
            ("t=1".into(), format!("t={}", HASH_ITERATIONS_MAX + 1)),
            ("p=1".into(), format!("p={}", HASH_LANES_MAX + 1)),
        ] {
            let hostile = costly.replacen(&from, &to, 1);
            assert_ne!(hostile, costly);
            assert!(!is_hash(&hostile), "{hostile}");
            assert!(!verify("correct horse battery", &hostile), "{hostile}");
        }
    }

    /// A stored hash with less memory, fewer iterations or an older version than the console's
    /// own is weaker, and redone at the next sign-in; the console's own, or a costlier one within
    /// the bounds, is not; a string that is no stored hash is not either (it admits nothing).
    #[test]
    fn a_hash_below_the_console_s_parameters_is_weaker() {
        let salt = SaltString::generate(&mut OsRng);
        let with = |version: Version, m: u32, t: u32, p: u32| {
            let params = Params::new(m, t, p, None).unwrap();
            let argon2 = Argon2::new(argon2::Algorithm::Argon2id, version, params);
            let phc = argon2.hash_password(b"correct horse battery", &salt);
            phc.unwrap().to_string()
        };
        let own = hash("correct horse battery").unwrap();
        assert!(!weaker(&own));
        assert!(!weaker(dummy()));
        let (m, t, p) = (ARGON2_MEMORY_KIB, ARGON2_ITERATIONS, ARGON2_LANES);
        assert!(!weaker(&with(Version::V0x13, m + 1024, t + 1, p + 1)));
        for weak in [
            with(Version::V0x13, m - 1024, t, p),
            with(Version::V0x13, m, t - 1, p),
            with(Version::V0x10, m, t, p),
        ] {
            assert!(is_hash(&weak) && weaker(&weak), "{weak}");
            assert!(verify("correct horse battery", &weak), "{weak}");
        }
        assert!(!weaker("admin123") && !weaker(""));
    }
}
