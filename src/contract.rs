//! The contract-kind interface: each kind turns pack output into provider and consumer
//! contracts and matches them into links (ADR 0005). HTTP is the only kind in v0.1; later
//! kinds (`channel::`, `package::`, `resource::`) plug in behind the same trait.

use serde::Serialize;

use crate::http::Http;
use crate::index::{Consumer, Provider};
use crate::lang::Facts;
use crate::workspace::Member;

/// How well a link is established. Strongest first: exact, inferred, ambiguous (ADR 0005 adds
/// declared and decided with later releases).
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Exact,
    Inferred,
    Ambiguous,
}

/// A consumer call site linked to a provider symbol through a contract.
#[derive(Serialize)]
pub struct Link {
    pub consumer: String,
    pub provider: String,
    pub contract: String,
    pub level: Level,
    pub reason: String,
}

/// A contract one file holds, before the index gives its site an identity.
pub struct Found {
    pub site: Site,
    /// The contract identifier; `None` for a consumer Meada could not resolve.
    pub contract: Option<String>,
    /// How a consumer names the provider's repository, such as `host:api.example.com`.
    pub via: Option<String>,
}

pub enum Site {
    /// A provider: a definition of the file, by qualified name, or the file itself.
    Def(String),
    File,
    /// A consumer: the call at this position of `Facts::calls`.
    Call(usize),
}

pub trait ContractKind {
    /// The identifier prefix: `http` for `http::GET::/users`.
    fn prefix(&self) -> &'static str;

    /// The contracts the file at the repository-relative `path` provides and consumes, read
    /// from its language pack's facts.
    fn extract(&self, path: &str, facts: &Facts) -> Vec<Found>;

    /// The links from `consumer`, held by the repository `from`, to the providers of the
    /// workspace (each with the repository holding it). Several fitting providers are all
    /// linked as ambiguous; none leaves the consumer unlinked.
    fn link(
        &self,
        consumer: &Consumer,
        from: &Member,
        providers: &[(&Member, &Provider)],
    ) -> Vec<Link>;
}

/// Every contract kind of this release.
pub const KINDS: &[&dyn ContractKind] = &[&Http];

/// The kind of a contract identifier, from its prefix.
pub fn kind_of(contract: &str) -> Option<&'static dyn ContractKind> {
    let (prefix, _) = contract.split_once("::")?;
    KINDS.iter().copied().find(|k| k.prefix() == prefix)
}
