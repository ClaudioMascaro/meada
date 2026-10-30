//! The canonical export of one repository (`meada export`), as it is written: the binary writes
//! it through these types and the console's graph views read it back with them, so a parsed
//! export written again is the same bytes. What the build needs and never writes (where the
//! repository stands, the manifests read, the gateways of a route) stays in the binary.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::shape::{Drift, Shape};

/// How well a link is established. The order is the strength, strongest first: exact >
/// declared > inferred > decided > ambiguous (ADR 0005), so a chain takes its greatest
/// (weakest) level.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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

/// One repository's index content with the links from its consumers and to its providers, every
/// list sorted. A list that is empty is left out, but for the first six and `links`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Export {
    pub repository: String,
    pub files: Vec<File>,
    pub symbols: Vec<Symbol>,
    pub calls: Vec<CallSite>,
    pub providers: Vec<Provider>,
    pub consumers: Vec<Consumer>,
    /// The RabbitMQ bindings the repository declares (spec #20), neither side of a channel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,
    /// The provider documents read (spec #21), in path order, with their contracts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<Document>,
    /// The RabbitMQ publishes to a named exchange (spec #20), producers of the queues the
    /// bindings route them to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publishes: Vec<Publish>,
    /// The packages the repository provides, by name, then directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<Package>,
    /// The dependency entries naming a package of the workspace, by manifest, field, name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Dependency>,
    /// The imports resolved into a package of the workspace, by call site, then target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<Import>,
    /// The repository's communities (spec #31), by name, each with its sorted members.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub communities: Vec<Community>,
    pub links: Vec<Link>,
    /// The process of each entry point (spec #31), in entry point order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub processes: Vec<Process>,
    /// The repository's decisions that do not apply, each with its cause.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_decisions: Vec<Stale>,
}

impl Export {
    /// The export as `meada export` prints it: pretty-printed JSON and a newline.
    pub fn write(&self) -> serde_json::Result<String> {
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct File {
    pub path: String,
    pub hash: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Span {
    pub path: String,
    pub start: i64,
    pub end: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Symbol {
    pub id: String,
    pub span: Span,
    /// The BLAKE3 of the span's exact bytes, in hex.
    #[serde(default)]
    pub hash: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct CallSite {
    pub id: String,
    pub caller: String,
    pub target: Option<String>,
    pub level: Option<Level>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    pub span: Span,
    /// An `inferred` name match narrowed to one of several definitions of its name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub narrowed: bool,
    /// The definition an annotation, attribute or decorator call decorates; absent for an
    /// ordinary call. The caller stays the enclosing scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorates: Option<String>,
}

/// The provider side of a contract: a symbol that serves it, a file, a repository, the call
/// that produces to a channel, or a route or channel side only a document lists
/// (`<repository>::<document>::<METHOD> <path>`, or `... <broker> send <name>`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Provider {
    pub contract: String,
    pub site: String,
    /// The path a gateway serves the route at, when `[serve]` rewrites it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    /// What the route's response or the channel's messages carry: from the first document by
    /// path that gives one, else from the code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

/// The consumer side of a contract: a call site that uses it, or the symbol that reads a
/// resource or handles a channel; `contract` is `None` while an HTTP call is unresolved. `via` says
/// how it names the provider's repository, such as `host:api.example.com` or `env:API_URL`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Consumer {
    pub contract: Option<String>,
    pub site: String,
    pub via: Option<String>,
    /// What the messages a channel handler receives carry; for an HTTP call, the fields it
    /// reads from the parsed response (spec #23).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

/// A queue bound to an exchange by a pattern: the call `site` (a call site) that binds the
/// queue channel `contract`. A publish to the exchange reaches the queue when the pattern
/// matches its routing key.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Binding {
    pub contract: String,
    pub exchange: String,
    pub pattern: String,
    pub site: String,
}

/// A publish to the named exchange `exchange` with the routing key `key`, at the call `site`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Publish {
    pub exchange: String,
    pub key: String,
    pub site: String,
}

/// The kind of a document read, named by its top-level field: `openapi` or `asyncapi`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Openapi,
    Asyncapi,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Openapi => "openapi",
            Format::Asyncapi => "asyncapi",
        }
    }
}

/// A provider document read: its path, format, version and contracts, sorted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Document {
    pub path: String,
    pub format: Format,
    pub version: Option<String>,
    pub contracts: BTreeSet<String>,
}

impl Document {
    /// Every current field, in declaration order. A field added later is optional: it starts
    /// empty and is set after `new`.
    pub fn new(
        path: String,
        format: Format,
        version: Option<String>,
        contracts: BTreeSet<String>,
    ) -> Self {
        Self {
            path,
            format,
            version,
            contracts,
        }
    }
}

/// A package a repository provides.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Package {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub repository: String,
    /// The directory of the manifest, relative to the repository root: `.` for the root.
    pub directory: String,
    /// Its ecosystem, given only when it is not npm's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ecosystem: Option<String>,
}

/// A dependency entry that names a package of the workspace.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[non_exhaustive]
pub struct Dependency {
    /// The path of the manifest holding the entry.
    pub manifest: String,
    pub field: String,
    pub name: String,
    /// Cargo only: the key the entry renames the package to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
}

/// What resolved an import in a provider: the declared field or rule that named the entry, or
/// `OutDir` when the output-to-source mapping found the file. For an `ambiguous` import, the
/// field or rule whose entry could not be followed to a source file. A PyPI import is named by
/// the module it imports: the distribution's own name (`ModuleName`) or one of its top-level
/// modules (`TopLevel`); a Rust `use` by the crate's name (`CrateName`); a Go import by the
/// module path it starts with (`ModulePath`); a Java import by the package that declares its
/// class (`Package`); a C# `using` by the namespace that declares its type (`Namespace`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum How {
    Exports,
    Types,
    Module,
    Main,
    Path,
    OutDir,
    ModuleName,
    TopLevel,
    CrateName,
    ModulePath,
    Package,
    Namespace,
}

impl How {
    pub fn name(self) -> &'static str {
        match self {
            How::Exports => "exports",
            How::Types => "types",
            How::Module => "module",
            How::Main => "main",
            How::Path => "path",
            How::OutDir => "outDir",
            How::ModuleName => "module name",
            How::TopLevel => "top-level module",
            How::CrateName => "crate name",
            How::ModulePath => "module path",
            How::Package => "package",
            How::Namespace => "namespace",
        }
    }
}

/// An import resolved into a package of the workspace.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Import {
    /// The call through the imported binding.
    pub site: String,
    pub specifier: String,
    /// What the call names of the import.
    pub name: String,
    pub level: Level,
    pub how: How,
    pub target: String,
}

impl Import {
    /// Every current field, in declaration order. A field added later is optional: it starts
    /// empty and is set after `new`.
    pub fn new(
        site: String,
        specifier: String,
        name: String,
        level: Level,
        how: How,
        target: String,
    ) -> Self {
        Self {
            site,
            specifier,
            name,
            level,
            how,
            target,
        }
    }
}

/// A community of one repository: its name and its members, sorted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Community {
    pub name: String,
    pub members: Vec<String>,
}

/// A consumer linked to a provider through a contract. Each side is a call site, a symbol, a
/// file or a whole repository, as its kind places it: an HTTP consumer is a call and its
/// provider a symbol; a channel consumer is its handler and its producer a call.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Link {
    pub consumer: String,
    pub provider: String,
    pub contract: String,
    pub level: Level,
    pub reason: String,
    /// Who decided a `decided` link: `human` or a model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// The probability a model stated for a `decided` link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p: Option<f64>,
    /// The shape drift of an HTTP link with a shape on both sides (spec #23, ADR 0009).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drift: Option<Drift>,
}

impl Link {
    /// Every current field, in declaration order. A field added later is optional: it starts
    /// empty and is set after `new`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        consumer: String,
        provider: String,
        contract: String,
        level: Level,
        reason: String,
        by: Option<String>,
        p: Option<f64>,
        drift: Option<Drift>,
    ) -> Self {
        Self {
            consumer,
            provider,
            contract,
            level,
            reason,
            by,
            p,
            drift,
        }
    }
}

/// What kind of door an entry point is (spec #31).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    /// A route an HTTP handler serves.
    Route,
    /// An exported function or method nothing in its repository calls.
    Export,
    /// A channel a consumer handler consumes, one entry point per channel.
    Channel,
    /// A program's start, as its language pack declares it.
    Program,
}

impl EntryKind {
    /// Every kind, in the order a symbol's process summary lists them.
    pub const ALL: [EntryKind; 4] = [
        EntryKind::Route,
        EntryKind::Export,
        EntryKind::Channel,
        EntryKind::Program,
    ];

    /// The name the index stores and the export shows.
    pub fn name(self) -> &'static str {
        match self {
            EntryKind::Route => "route",
            EntryKind::Export => "export",
            EntryKind::Channel => "channel",
            EntryKind::Program => "program",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

/// The process of one entry point: the steps `trace` shows for it, depth first.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Process {
    pub entry_point: String,
    pub kind: EntryKind,
    pub label: String,
    pub steps: Vec<Step>,
}

impl Process {
    /// Every current field, in declaration order. A field added later is optional: it starts
    /// empty and is set after `new`.
    pub fn new(entry_point: String, kind: EntryKind, label: String, steps: Vec<Step>) -> Self {
        Self {
            entry_point,
            kind,
            label,
            steps,
        }
    }
}

/// One step of a process.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Step {
    pub depth: usize,
    pub repository: String,
    /// The level of the call or link that reached it.
    pub level: Option<Level>,
    pub via: Via,
    /// What the step is at: a symbol, a file, a repository or a side only a document lists, or
    /// for a note, the call site or symbol it is about.
    pub node: String,
    /// The reason of the link that reached the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<Mark>,
    /// The candidates of an ambiguous call or link.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
}

impl Step {
    /// Every current field, in declaration order. A field added later is optional: it starts
    /// empty and is set after `new`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        depth: usize,
        repository: String,
        level: Option<Level>,
        via: Via,
        node: String,
        reason: Option<String>,
        mark: Option<Mark>,
        candidates: Vec<String>,
        contract: Option<String>,
    ) -> Self {
        Self {
            depth,
            repository,
            level,
            via,
            node,
            reason,
            mark,
            candidates,
            contract,
        }
    }
}

/// How a step of a process was reached.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// It is the entry point itself.
    Entry,
    /// A call inside a repository, or a note under one.
    Call,
    Link,
}

/// What sets a step apart from a plain node followed once.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Mark {
    /// The node is already on the current branch: shown, not expanded.
    Cycle,
    /// The node was expanded earlier in the process: shown as a reference.
    Seen,
    /// An ambiguous call or link: one step listing its candidates, not expanded.
    Ambiguous,
    /// A consumer whose contract Meada could not read (`node` is its call site).
    Unresolved,
    /// A consumer of `contract` that no provider serves (`node` is its call site).
    NoProvider,
    /// A channel side Meada could not name, which links to nothing (`node` is its site).
    Produces,
    Consumes,
    /// The branch is at the process depth limit: shown, not expanded.
    DepthLimit,
}

impl Mark {
    /// The verb of the channel side a mark stands for, `Produces` or `Consumes`.
    pub fn verb(self) -> &'static str {
        match self {
            Mark::Produces => "produces",
            _ => "consumes",
        }
    }
}

/// Why a decision does not apply. A decision that waits on a missing peer has none: it is
/// neither applied nor stale.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    /// No call with the decision's `call` id is in the repository.
    CallGone,
    /// The call is there, but the core resolves it on its own or it is no item of the
    /// decision's kind: nothing is left to decide.
    ItemNotOpen,
    /// No repository of the workspace serves the target's contract.
    TargetGone,
    /// The target is served, but is not among the candidates of the ambiguous link.
    NotCandidate,
}

/// A decision that does not apply.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Stale {
    pub call: String,
    pub target: String,
    pub cause: Cause,
}

impl Stale {
    /// Every current field, in declaration order. A field added later is optional: it starts
    /// empty and is set after `new`.
    pub fn new(call: String, target: String, cause: Cause) -> Self {
        Self {
            call,
            target,
            cause,
        }
    }
}

/// The export written as `text`, or why it does not fit the export types: where in the document,
/// then serde's reason, such as ``calls[3].span: missing field `start` `` or
/// ``files[0].hash: invalid type: integer `7`, expected a string``.
pub fn parse(text: &str) -> Result<Export, String> {
    serde_json::from_str(text).map_err(|e| {
        let reason = e.to_string();
        if !e.is_data() {
            return reason;
        }
        // serde_json ends its reason with the line and column; the place says where instead.
        let reason = reason.rsplit_once(" at line ").map_or(&*reason, |(r, _)| r);
        match place(text, offset(text, e.line(), e.column())) {
            place if place.is_empty() => reason.to_string(),
            place => format!("{place}: {reason}"),
        }
    })
}

/// The byte offset just past the 1-based `line` and `column` serde_json reports.
fn offset(text: &str, line: usize, column: usize) -> usize {
    let before: usize = (text.split_inclusive('\n').take(line.saturating_sub(1)))
        .map(str::len)
        .sum();
    (before + column).min(text.len())
}

/// The path to the value being read at byte `end` of the JSON `text`: each open object by the
/// key last read in it, each open array by the index of its element (`calls[3].span.start`),
/// empty at the top. A missing field is reported past the object lacking it, so its path is that
/// object's.
fn place(text: &str, end: usize) -> String {
    enum Open {
        Object(Option<String>),
        Array(usize),
    }
    let mut stack: Vec<Open> = Vec::new();
    let bytes = text.as_bytes();
    // Whether the next string is a key: right after `{`, or after `,` in an object.
    let mut key_next = false;
    let mut i = 0;
    while i < end.min(bytes.len()) {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                if let (true, Some(Open::Object(key))) = (key_next, stack.last_mut()) {
                    let raw = text.get(start..i).unwrap_or_default();
                    let unescaped = serde_json::from_str(&format!("\"{raw}\""));
                    *key = Some(unescaped.unwrap_or_else(|_| raw.to_string()));
                }
                key_next = false;
            }
            b'{' => {
                stack.push(Open::Object(None));
                key_next = true;
            }
            b'[' => stack.push(Open::Array(0)),
            b'}' | b']' => {
                stack.pop();
                key_next = false;
            }
            b',' => match stack.last_mut() {
                Some(Open::Array(n)) => *n += 1,
                Some(Open::Object(_)) => key_next = true,
                None => {}
            },
            _ => {}
        }
        i += 1;
    }
    let mut path = String::new();
    for open in &stack {
        match open {
            Open::Object(Some(key)) if path.is_empty() => path += key,
            Open::Object(Some(key)) => path += &format!(".{key}"),
            Open::Object(None) => {}
            Open::Array(n) => path += &format!("[{n}]"),
        }
    }
    path
}
