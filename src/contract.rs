//! The contract-kind interface: each kind turns pack output into provider and consumer
//! contracts and matches them into links (ADR 0005). HTTP is the only kind in v0.1; later
//! kinds (`channel::`, `package::`, `resource::`) plug in behind the same trait.

use std::collections::HashMap;

use serde::Serialize;

use crate::http::Http;
use crate::index::{Consumer, Provider};
use crate::resolve::Code;
use crate::workspace::Member;

/// How well a link is established. The order is the strength, strongest first: exact >
/// inferred > ambiguous, so a chain takes its greatest (weakest) level. ADR 0005 adds declared
/// and decided with later releases.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Exact,
    Inferred,
    Ambiguous,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Exact => "exact",
            Level::Inferred => "inferred",
            Level::Ambiguous => "ambiguous",
        }
    }

    /// The level `name` names, as the index stores call levels.
    pub fn parse(name: &str) -> Option<Self> {
        [Level::Exact, Level::Inferred, Level::Ambiguous]
            .into_iter()
            .find(|l| l.name() == name)
    }
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

/// A contract the repository holds, before the index gives its site an identity.
pub struct Found {
    pub site: Site,
    /// The contract identifier; `None` for a consumer Meada could not resolve.
    pub contract: Option<String>,
    /// How a consumer names the provider's repository, such as `host:api.example.com`.
    pub via: Option<String>,
}

/// Where a contract sits; every path is repository-relative.
pub enum Site {
    /// A provider: a definition, by qualified name.
    Def { path: String, name: String },
    /// A provider: the file itself.
    File(String),
    /// A provider: an inline route handler, the `Kind::Inline` definition `def`, which takes
    /// `name` (such as `GET /users/{}`) under its enclosing symbol.
    Inline {
        path: String,
        def: String,
        name: String,
    },
    /// A consumer: the call at this position of the file's `Facts::calls`.
    Call { path: String, index: usize },
}

pub trait ContractKind {
    /// The identifier prefix: `http` for `http::GET::/users`.
    fn prefix(&self) -> &'static str;

    /// The contracts one language pack's files provide and consume, read from the pack's
    /// facts and the values `code` evaluates.
    fn extract(&self, code: &Code) -> Vec<Found>;

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

/// The names of `Kind::Inline` definitions (inline route handlers, spec #3): the name a kind
/// gave, with an ordinal `#<n>` from the second equal name under one enclosing symbol on, in
/// source order. Every other inline definition folds into its enclosing one, as if it were
/// not there.
#[derive(Default)]
pub struct Names {
    /// `(path, "@<offset>")` to the segment that replaces it.
    named: HashMap<(String, String), String>,
}

impl Names {
    pub fn new(found: &[Found]) -> Self {
        let mut proposed: Vec<(&str, usize, &str, &str)> = Vec::new();
        for f in found {
            if let Site::Inline { path, def, name } = &f.site {
                let segment = def.rsplit('.').next().unwrap_or(def);
                let offset = segment
                    .trim_start_matches('@')
                    .parse()
                    .unwrap_or(usize::MAX);
                proposed.push((path, offset, def, name));
            }
        }
        // A handler mounted under several prefixes keeps its first name.
        proposed.sort_by_key(|&(path, offset, _, _)| (path, offset));
        proposed.dedup_by_key(|&mut (path, offset, _, _)| (path, offset));
        let mut names = Self::default();
        let mut taken: HashMap<(String, String, String), usize> = HashMap::new();
        // Source order: an enclosing handler is named before the handlers inside it.
        for (path, _, def, name) in proposed {
            let (parent, segment) = def.rsplit_once('.').unwrap_or(("", def));
            let parent = names.scope(path, parent);
            let n = taken.entry((path.into(), parent, name.into())).or_insert(0);
            let named = match *n {
                0 => name.to_string(),
                n => format!("{name}#{n}"),
            };
            *n += 1;
            names.named.insert((path.into(), segment.into()), named);
        }
        names
    }

    /// The name of the definition `name` of `path`; `None` for an inline one that folds.
    pub fn def(&self, path: &str, name: &str) -> Option<String> {
        let last = name.rsplit('.').next().unwrap_or(name);
        if last.starts_with('@') && !self.named.contains_key(&(path.into(), last.into())) {
            return None;
        }
        Some(self.scope(path, name))
    }

    /// A qualified name of `path` with each inline definition in it named or folded away.
    pub fn scope(&self, path: &str, scope: &str) -> String {
        if !scope.contains('@') {
            return scope.to_string();
        }
        scope
            .split('.')
            .filter_map(|segment| match segment.starts_with('@') {
                true => self.named.get(&(path.into(), segment.into())).cloned(),
                false => Some(segment.to_string()),
            })
            .collect::<Vec<_>>()
            .join(".")
    }
}
