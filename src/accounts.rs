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

// The console (feature `ui`) is the one reader of the keys, the policy and the store so far.
#![cfg_attr(not(feature = "ui"), allow(dead_code))]

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
/// How long a session token lives.
pub const SESSION_HOURS: u64 = 8;
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
        Ok(Config {
            public_key,
            private_key,
            admin_password: admin_password.unwrap_or(Secret::Env(ADMIN_PASSWORD_ENV.into())),
            break_glass: self.break_glass,
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

impl Config {
    /// The signing key pair, read now: the console's start check. Each half must be named, read,
    /// Ed25519 in PEM, and the private half must sign what the public half verifies; a problem
    /// names its key and never the value.
    pub fn keys(&self) -> Result<Keys, Problem> {
        const PUBLIC: (&str, &str) = ("accounts.public_key_env", "accounts.public_key_file");
        const PRIVATE: (&str, &str) = ("accounts.private_key_env", "accounts.private_key_file");
        let named = |secret: &Option<Secret>, keys: (&'static str, &'static str), private| {
            secret.clone().ok_or_else(|| {
                let missing = Msg::AccountsKeyMissing {
                    env: keys.0,
                    file: keys.1,
                    private,
                };
                ("accounts", missing.text())
            })
        };
        let public = named(&self.public_key, PUBLIC, false)?;
        let private = named(&self.private_key, PRIVATE, true)?;
        let public_key = key_of(&public, PUBLIC.0, PUBLIC.1);
        let private_key = key_of(&private, PRIVATE.0, PRIVATE.1);
        let not_ed25519 = |key, private| (key, Msg::AccountsNotEd25519 { private }.text());
        let decoding = DecodingKey::from_ed_pem(read(&public, PUBLIC, KEY_MAX)?.as_bytes())
            .map_err(|_| not_ed25519(public_key, false))?;
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
        if jsonwebtoken::decode::<Probe>(&probe, &decoding, &validation).is_err() {
            return Err((private_key, Msg::AccountsKeyMismatch.text()));
        }
        Ok(Keys { encoding, decoding })
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
    /// Set only while the password must be changed: the token then opens the change alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub change_required: bool,
    pub iat: u64,
    pub exp: u64,
}

impl Claims {
    /// The username the subject names.
    pub fn username(&self) -> &str {
        username_of(&self.sub).unwrap_or_default()
    }
}

/// The signing key pair, the console's alone.
pub struct Keys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl Keys {
    /// A session token for `username` from `issuer`, now: the token and its expiry in seconds
    /// since the epoch.
    pub fn session(
        &self,
        issuer: &str,
        username: &str,
        role: Role,
        groups: &[String],
        change_required: bool,
    ) -> Result<(String, u64), String> {
        let iat = crate::modules::now();
        let exp = iat + SESSION_HOURS * 3600;
        let claims = Claims {
            iss: issuer.to_string(),
            aud: CONSOLE.to_string(),
            sub: subject(username),
            role,
            groups: groups.to_vec(),
            change_required,
            iat,
            exp,
        };
        let header = Header::new(Algorithm::EdDSA);
        let token = jsonwebtoken::encode(&header, &claims, &self.encoding);
        Ok((token.map_err(|e| e.to_string())?, exp))
    }

    /// The claims of `token` when it passes: EdDSA only, no `crit`, signed by this key pair,
    /// issued by `issuer` for `audience`, `exp` and `nbf` with the identity provider's leeway, a
    /// subject `local:<username>`, a role and groups that are owner names. Nothing says why a
    /// token fails: every failure is the same 401.
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
        let claims = jsonwebtoken::decode::<Claims>(token, &self.decoding, &validation).ok()?;
        let claims = claims.claims;
        (username_of(&claims.sub).is_some() && groups_ok(&claims.groups)).then_some(claims)
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
            .session(issuer, "alice", Role::Viewer, &groups, true)
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
            .session(issuer, "alice", Role::Operator, &[], false)
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
        for edit in [
            ("exp", serde_json::json!(now - 2 * crate::oidc::LEEWAY)),
            ("nbf", serde_json::json!(now + 2 * crate::oidc::LEEWAY)),
            ("sub", serde_json::json!("alice")),
            ("sub", serde_json::json!("local:Alice")),
            ("role", serde_json::json!("root")),
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

    /// A bearer is routed by its issuer read without a check: this console's session token by
    /// its own issuer, never another's; what is no JWT, or has no issuer, by none.
    #[test]
    fn a_token_is_routed_by_its_unchecked_issuer() {
        let (private, public) = pair();
        let keys = keyed("issued", &public, &private).keys().unwrap();
        let issuer = "https://console.example";
        let (token, _) = keys
            .session(issuer, "admin", Role::Operator, &[], false)
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
