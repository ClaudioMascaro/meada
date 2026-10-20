//! The contract-kind interface: each kind turns pack output, or what `meada.toml` declares,
//! into provider and consumer contracts and matches them into links (ADR 0005). The kinds are
//! HTTP (`http::`), resources (`resource::`, spec #19), channels (`channel::`, spec #20) and
//! packages (`package::`, spec #22); later kinds plug in behind the same trait.

use std::collections::HashMap;

use serde::Serialize;

use crate::channel::Channel;
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

/// The engine fixes the phase order, and the kinds answer in `KINDS` order: every kind's
/// `declare` over every repository, then every kind's `workspace_step` over all of them, then
/// `link`, then the decisions replay, then every kind's `after_links`. The decisions are
/// kind-free and a pass over the links must see the decided ones, so they sit between. A new
/// kind appends to `KINDS`: the order of the list is the order of the steps and of the output.
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

    /// Runs once over the index content of every repository of the workspace (in workspace
    /// order), after every kind's `declare` has run on every repository: a step that needs
    /// all of them, such as an index of the providers of the whole workspace.
    fn workspace_step(&self, _exports: &mut [Export]) {}

    /// The words for a provider and a consumer of this kind when `context` lists a contract
    /// by its sides, such as `("writer", "reader")`; `None` for a kind it does not (routes).
    fn sides(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    /// The links from `consumer`, held by the repository `from`, to the providers of the
    /// workspace (each with the repository holding it) and, for a queue, the publishes the
    /// bindings of the workspace's `exports` route to it. Several fitting providers are all
    /// linked as ambiguous; none leaves the consumer unlinked.
    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
        exports: &[Export],
    ) -> Vec<Link>;

    /// Runs once over the links of the whole workspace, after the decisions replay, so it sees
    /// the decided links: a pass that sets something on them, such as the drift of a link.
    fn after_links(&self, _links: &mut [Link], _exports: &[Export]) {}

    /// Whether the identifier is an unresolved side: one whose name could not be evaluated
    /// (a channel that is only its broker prefix). It never links, is no open item, and is
    /// shown with the unresolved suffix.
    fn unresolved(&self, _contract: &str) -> bool {
        false
    }

    /// How the identifier is labelled in candidates and entry points. It is a function of the
    /// identifier alone, since entry points are stored when the index is written.
    fn label(&self, contract: &str) -> String {
        shown(contract)
    }

    /// The status lines of `repository` (whose index content is `export`) about this kind,
    /// given the links of the workspace.
    fn status(&self, _repository: &Repository, _export: &Export, _links: &[Link]) -> Vec<String> {
        Vec::new()
    }
}

/// Every contract kind of this release.
pub const KINDS: &[&dyn ContractKind] = &[&Http, &Resource, &Channel, &Packages];

/// The kind of a contract identifier, from its prefix.
pub fn kind_of(contract: &str) -> Option<&'static dyn ContractKind> {
    let (prefix, _) = contract.split_once("::")?;
    KINDS.iter().copied().find(|k| k.prefix() == prefix)
}

/// Whether `contract` is an unresolved side, by the rule of its kind.
pub fn unresolved(contract: &str) -> bool {
    kind_of(contract).is_some_and(|k| k.unresolved(contract))
}

/// `contract` as a person reads it: an unresolved side says so.
pub fn shown(contract: &str) -> String {
    match unresolved(contract) {
        true => format!("{contract} (unresolved)"),
        false => contract.to_string(),
    }
}

/// The contract a person typed as `shown` printed it: `channel::kafka (unresolved)` is
/// `channel::kafka`.
pub fn typed(text: &str) -> &str {
    match text.strip_suffix(" (unresolved)") {
        Some(contract) if unresolved(contract) => contract,
        _ => text,
    }
}

/// `contract` as its kind labels it in candidates and entry points.
pub fn label(contract: &str) -> String {
    kind_of(contract).map_or_else(|| shown(contract), |k| k.label(contract))
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

#[cfg(test)]
mod tests {
    use super::{
        Channel, ContractKind, Http, Level, Link, Packages, Resource, shown, typed, unresolved,
    };
    use crate::config::Config;
    use crate::decisions::Decisions;
    use crate::entry::{EntryKind, EntryPoint};
    use crate::index::Export;
    use crate::workspace::Repository;

    /// The seam shows an unresolved side with its suffix and reads the suffix back off.
    #[test]
    fn shown_and_typed_round_trip_through_the_kind() {
        let broker_only = "channel::kafka";
        assert!(unresolved(broker_only));
        assert_eq!(shown(broker_only), "channel::kafka (unresolved)");
        assert_eq!(typed("channel::kafka (unresolved)"), broker_only);

        let named = "channel::kafka::orders.created";
        assert!(!unresolved(named));
        assert_eq!(shown(named), named);
        assert_eq!(typed(named), named);

        // The suffix means nothing on a resolved side or on a kind with no unresolved sides.
        assert_eq!(
            typed("channel::kafka::x (unresolved)"),
            "channel::kafka::x (unresolved)"
        );
        assert_eq!(shown("http::GET::/users"), "http::GET::/users");
        assert_eq!(shown("not a contract"), "not a contract");
    }

    fn resource_repo(toml: &str) -> Repository {
        Repository {
            root: "".into(),
            identity: "r".into(),
            config: toml::from_str::<Config>(toml).unwrap(),
            decisions: Decisions::default(),
        }
    }

    fn entry(identity: &str, kind: EntryKind) -> EntryPoint {
        EntryPoint {
            identity: identity.into(),
            kind,
            label: identity.into(),
            site: identity.into(),
        }
    }

    /// A resource no link reaches is reported by its kind; a kind with no lines of its own
    /// reports none.
    #[test]
    fn resources_report_status_lines_and_other_kinds_none() {
        let repo =
            resource_repo("[[resource]]\nkind = \"postgres\"\nid = \"orders\"\nrole = \"read\"\n");
        let export = Export::default();
        let lines = Resource.status(&repo, &export, &[]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("resource::postgres::orders"), "{lines:?}");
        assert!(Http.status(&repo, &export, &[]).is_empty());
        assert!(Channel.status(&repo, &export, &[]).is_empty());
    }

    /// A workspace step over a workspace with no package imports leaves every entry point as
    /// it is, whichever kind runs it (the monorepo rule itself is pinned by the export and
    /// process goldens).
    #[test]
    fn workspace_steps_keep_the_entry_points_of_a_workspace_without_imports() {
        let mut export = Export {
            repository: "r".into(),
            ..Export::default()
        };
        export.entry_points = vec![
            entry("r::a.ts::free", EntryKind::Export),
            entry("r::a.ts::route", EntryKind::Route),
        ];
        let mut exports = vec![export];
        for kind in [&Packages as &dyn ContractKind, &Http, &Resource, &Channel] {
            kind.workspace_step(&mut exports);
        }
        let kept: Vec<_> = (exports[0].entry_points.iter())
            .map(|e| &e.identity[..])
            .collect();
        assert_eq!(kept, ["r::a.ts::free", "r::a.ts::route"]);
    }

    /// The HTTP pass after links sets nothing on a link with no shapes; a kind with no pass
    /// leaves the links alone.
    #[test]
    fn the_pass_after_links_leaves_a_link_without_shapes_alone() {
        let mut links = vec![Link {
            consumer: "r::a.ts::c".into(),
            provider: "s::b.ts::p".into(),
            contract: "http::GET::/x".into(),
            level: Level::Exact,
            reason: "x".into(),
            by: None,
            p: None,
            drift: None,
        }];
        Http.after_links(&mut links, &[]);
        Channel.after_links(&mut links, &[]);
        assert!(links[0].drift.is_none());
    }
}
