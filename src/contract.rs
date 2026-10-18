//! The contract-kind interface: each kind turns pack output, or what `meada.toml` declares,
//! into provider and consumer contracts and matches them into links (ADR 0005). The kinds are
//! HTTP (`http::`), resources (`resource::`, spec #19), channels (`channel::`, spec #20) and
//! packages (`package::`, spec #22); later kinds plug in behind the same trait.

use std::collections::HashMap;

use serde::Serialize;

use crate::channel::{Channel, Routes};
use crate::http::Http;
use crate::index::{Consumer, Export, Provider};
use crate::package::Packages;
use crate::resolve::Code;
use crate::resource::Resource;
use crate::shape::{Drift, Shape};
use crate::workspace::Repository;

/// How well a link is established. The order is the strength, strongest first: exact >
/// declared > inferred > decided > ambiguous (ADR 0005), so a chain takes its greatest
/// (weakest) level.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Exact,
    /// From a declaration in `meada.toml`, such as a shared resource (spec #19).
    Declared,
    Inferred,
    /// From `meada.decisions.toml` (ADR 0006).
    Decided,
    Ambiguous,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Exact => "exact",
            Level::Declared => "declared",
            Level::Inferred => "inferred",
            Level::Decided => "decided",
            Level::Ambiguous => "ambiguous",
        }
    }

    /// The level `name` names, as the index stores call levels.
    pub fn parse(name: &str) -> Option<Self> {
        [
            Level::Exact,
            Level::Declared,
            Level::Inferred,
            Level::Decided,
            Level::Ambiguous,
        ]
        .into_iter()
        .find(|l| l.name() == name)
    }
}

/// A consumer linked to a provider through a contract. Each side is a call site, a symbol, a
/// file or a whole repository, as its kind places it: an HTTP consumer is a call and its
/// provider a symbol; a channel consumer is its handler and its producer a call.
#[derive(Serialize)]
pub struct Link {
    pub consumer: String,
    pub provider: String,
    pub contract: String,
    pub level: Level,
    pub reason: String,
    /// Who decided a `decided` link: `human` or a model id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// The probability a model stated for a `decided` link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<f64>,
    /// The drift of an HTTP link with a shape on both sides (spec #23, `http::drift`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift: Option<Drift>,
}

/// A contract the repository holds, before the index gives its site an identity.
pub struct Found {
    pub role: Role,
    pub site: Site,
    /// The contract identifier; `None` for an HTTP consumer Meada could not resolve.
    pub contract: Option<String>,
    /// How a consumer names the provider's repository, such as `host:api.example.com`; for a
    /// binding or a publish, the exchange.
    pub via: Option<String>,
    /// For an unresolved consumer, the method it calls with when Meada could read it; for a
    /// binding, the pattern; for a publish, the routing key.
    pub method: Option<String>,
    /// For an unresolved consumer, the literal fragments of its URL path Meada could
    /// evaluate, in order.
    pub fragments: Vec<String>,
    /// The shape the code gives the side (spec #23): for an HTTP consumer, the fields it reads
    /// from the parsed response.
    pub shape: Option<Shape>,
}

/// Which side of a contract a site holds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Provider,
    Consumer,
    /// Neither side: a RabbitMQ binding of a queue (`contract`) to an exchange (`via`) by a
    /// pattern (`method`), which routes a publish to the queue (spec #20).
    Binding,
    /// A RabbitMQ publish to the named exchange `via` with the routing key `method`, no
    /// `contract`: it holds no channel of its own and produces to the queues the bindings
    /// route it to (spec #20).
    Publish,
    /// Neither side: a call through a binding imported from a bare specifier (`via`), naming
    /// `method` of it (`greet`, `default`, `Client.create`), no `contract`. Stored, never
    /// exported; at query time, one that resolves into a package of the workspace becomes a
    /// consumer of that package (spec #22).
    Import,
    /// Neither side: the shape a package's index keeps for a name a file exports (issue #175),
    /// the module-level constant or type `method` of the file, read for the view `via` names
    /// (`shape::Want`). Stored, never exported; at query time it fills the parts of the shapes
    /// that import the name.
    Shape,
}

impl Role {
    /// The name the index stores.
    pub fn name(self) -> &'static str {
        match self {
            Role::Provider => "provider",
            Role::Consumer => "consumer",
            Role::Binding => "binding",
            Role::Publish => "publish",
            Role::Import => "import",
            Role::Shape => "shape",
        }
    }
}

/// Where a contract sits; every path is repository-relative.
pub enum Site {
    /// A definition, by qualified name. Inline definitions no kind names fold away from it,
    /// and a name that folds to nothing is the file's top level.
    Def { path: String, name: String },
    /// The file itself.
    File(String),
    /// An inline handler, the `Kind::Inline` definition `def`, which takes `name` (such as
    /// `GET /users/{}` or `kafka orders.created`) under its enclosing symbol.
    Inline {
        path: String,
        def: String,
        name: String,
    },
    /// The call at this position of the file's `Facts::calls`.
    Call { path: String, index: usize },
}

pub trait ContractKind {
    /// The identifier prefix: `http` for `http::GET::/users`.
    fn prefix(&self) -> &'static str;

    /// The contracts one language pack's files provide and consume, read from the pack's
    /// facts and the values `code` evaluates.
    fn extract(&self, code: &Code) -> Vec<Found>;

    /// Adds to `export`, the index content of `repo`, the sides `repo` declares for this kind
    /// in its `meada.toml`, keeping the lists sorted: a kind fed by configuration rather than
    /// code. A kind also uses it to mark what `meada.toml` says about the sides it found. They
    /// derive at query time, so an edit to `meada.toml` needs no reindex.
    fn declare(&self, _repo: &Repository, _export: &mut Export) {}

    /// The words for a provider and a consumer of this kind when `context` lists a contract
    /// by its sides, such as `("writer", "reader")`; `None` for a kind it does not (routes).
    fn sides(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    /// The links from `consumer`, held by the repository `from`, to the providers of the
    /// workspace (each with the repository holding it) and, for a queue, the publishes the
    /// workspace's `routes` send to it. Several fitting providers are all linked as ambiguous;
    /// none leaves the consumer unlinked.
    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
        routes: &Routes,
    ) -> Vec<Link>;
}

/// Every contract kind of this release.
pub const KINDS: &[&dyn ContractKind] = &[&Http, &Resource, &Channel, &Packages];

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
