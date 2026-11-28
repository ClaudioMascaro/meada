use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use crate::Result;

pub mod csharp;
pub mod go;
pub mod java;
pub mod kotlin;
pub mod python;
pub mod rust;
pub mod swift;
pub mod ts;

pub trait LanguagePack {
    fn extensions(&self) -> &'static [&'static str];

    fn is_code(&self, _path: &str) -> bool {
        true
    }

    fn resolves_with(&self) -> Option<&'static dyn LanguagePack> {
        None
    }

    fn parse(&self, path: &str, src: &[u8]) -> Result<Facts>;

    fn parse_with(&self, path: &str, src: &[u8], _setting: &str) -> Result<Facts> {
        self.parse(path, src)
    }

    fn settings(&self, _configs: &[(&str, &[u8])]) -> Box<dyn Fn(&str) -> String> {
        Box::new(|_| String::new())
    }

    fn extend(&self, _files: &BTreeMap<&str, &Facts>) -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn config_files(&self) -> &'static [&'static str];

    fn config_inputs<'b>(
        &self,
        _configs: &[(&str, &[u8])],
        _read: &dyn Fn(&str) -> ConfigInput<'b>,
    ) {
    }

    fn join(&self, _configs: &[(&str, &[u8])], _files: &BTreeMap<&str, &Facts>) -> Join {
        Box::new(|_, _| {})
    }

    fn modules(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>)
    -> Result<Modules>;

    fn layout(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>) -> Result<Layout> {
        Ok(Layout {
            members: None,
            modules: self.modules(configs, files)?,
            published: None,
            programs: Vec::new(),
            reaches: None,
            shared: Vec::new(),
        })
    }

    fn receiver(&self) -> Option<&'static str> {
        None
    }

    fn members_in_scope(&self) -> bool {
        false
    }

    fn drops_external_uses(&self) -> bool {
        false
    }

    fn assigned_members(&self) -> bool {
        false
    }

    fn guesses_constructs(&self) -> bool {
        false
    }

    fn unordered_globs(&self) -> bool {
        false
    }

    fn combines_globs(&self) -> bool {
        self.unordered_globs()
    }

    fn static_imports_inherit(&self) -> bool {
        true
    }

    fn qualified_types(&self) -> bool {
        false
    }

    fn namespaced(&self) -> bool {
        true
    }

    fn base_receiver(&self) -> Option<&'static str> {
        None
    }

    fn unordered_bases(&self) -> bool {
        false
    }

    fn implicit_members(&self) -> &'static [&'static str] {
        &[]
    }

    fn known_outside(&self) -> KnownOutside {
        &[]
    }

    fn extension_functions(&self) -> bool {
        false
    }

    fn extension_methods(&self) -> bool {
        false
    }

    fn outside_blocks(&self) -> bool {
        false
    }

    fn fluent(&self) -> bool {
        false
    }

    fn strict(&self) -> bool {
        false
    }

    fn globals(&self) -> &'static [&'static str] {
        &[]
    }

    fn stars_bind(&self) -> bool {
        false
    }
}

pub type Modules = Box<dyn Fn(&str, &str) -> Rc<[String]>>;

pub type Join = Box<dyn Fn(&str, &mut Facts)>;

pub type Reaches = Box<dyn Fn(&str, &str) -> bool>;

pub struct Layout {
    pub members: Option<BTreeSet<String>>,
    pub modules: Modules,
    pub published: Option<BTreeSet<String>>,
    pub programs: Vec<(String, String)>,
    pub reaches: Option<Reaches>,
    pub shared: Vec<Vec<String>>,
}

#[derive(Clone)]
pub enum ConfigInput<'b> {
    Read(std::borrow::Cow<'b, [u8]>),
    Symlink,
    Missing,
}

const PACKS: &[&dyn LanguagePack] = &[
    &ts::TsJs,
    &python::Python,
    &rust::Rust,
    &go::Go,
    &java::Java,
    &kotlin::Kotlin,
    &csharp::CSharp,
    &swift::Swift,
];

pub fn names_config(pattern: &str, file: &str) -> bool {
    match pattern.split_once('*') {
        Some((start, ending)) => {
            let shortest = start.len() + ending.len() + usize::from(start.is_empty());
            file.len() >= shortest && file.starts_with(start) && file.ends_with(ending)
        }
        None => file == pattern,
    }
}

pub type KnownEntry = (&'static str, &'static [&'static str]);

pub type KnownOutside = &'static [KnownEntry];

pub fn pack_for(path: &str) -> Option<usize> {
    let (_, extension) = path.rsplit_once('.')?;
    PACKS
        .iter()
        .position(|p| p.extensions().contains(&extension))
        .filter(|&pack| PACKS[pack].is_code(path))
}

pub fn resolver_of(index: usize) -> usize {
    (PACKS[index].resolves_with())
        .and_then(|with| (PACKS.iter()).position(|p| p.extensions() == with.extensions()))
        .unwrap_or(index)
}

pub fn known_outside(language: &dyn LanguagePack) -> Vec<KnownEntry> {
    let with = |p: &&dyn LanguagePack| {
        (p.resolves_with()).is_some_and(|w| w.extensions() == language.extensions())
    };
    (std::iter::once(language).chain(PACKS.iter().copied().filter(with)))
        .flat_map(|p| p.known_outside().iter().copied())
        .collect()
}

pub fn hash_spans(facts: &mut Facts, src: &[u8]) {
    for def in &mut facts.defs {
        let text = src.get(def.range.clone()).unwrap_or_default();
        def.hash = blake3::hash(text).to_hex().to_string();
    }
    for declaration in &mut facts.declarations {
        let text = src.get(declaration.range.clone()).unwrap_or_default();
        declaration.hash = blake3::hash(text).to_hex().to_string();
    }
}

thread_local! {
    static VALUES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn lean<T>(_: &T) -> bool {
    !VALUES.get()
}

fn lean_or_empty<K, V>(map: &BTreeMap<K, V>) -> bool {
    lean(map) || map.is_empty()
}

fn lean_or_absent<T>(value: &Option<T>) -> bool {
    lean(value) || value.is_none()
}

pub fn with_values<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            VALUES.set(self.0);
        }
    }
    let _restore = Restore(VALUES.replace(true));
    f()
}

mod as_str {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(scope: &Arc<str>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(scope)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<str>, D::Error> {
        Ok(Arc::from(String::deserialize(d)?))
    }
}

pub fn pack(index: usize) -> &'static dyn LanguagePack {
    PACKS[index]
}

#[cfg(test)]
pub fn packs() -> impl Iterator<Item = &'static dyn LanguagePack> {
    PACKS.iter().copied()
}

#[derive(Default, Serialize, Deserialize)]
pub struct Facts {
    pub defs: Vec<Def>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overloads: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub constructors: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statics: Vec<String>,
    pub imports: Vec<Import>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loads: Vec<String>,
    pub exports: Vec<Export>,
    pub star: Star,
    pub calls: Vec<Call>,
    pub consts: Vec<Const>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declarations: Vec<Declaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_fields: Vec<EnvField>,
    #[serde(default, skip_serializing_if = "lean")]
    pub rebound: Vec<(String, Value)>,
    #[serde(default, skip_serializing_if = "lean")]
    pub mutable: HashMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locals: Vec<String>,
    pub receivers: Vec<Receiver>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ts_locals: BTreeMap<String, Vec<TsLocal>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ts_local_literal_writes: BTreeMap<String, BTreeSet<usize>>,
    #[serde(default, skip_serializing_if = "lean_or_empty")]
    pub ts_record_locals: BTreeMap<String, Vec<TsLocal>>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub ts_annotated_literals: BTreeSet<usize>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub ts_global_calls: BTreeSet<(usize, usize)>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub ts_rebound_this: BTreeSet<(usize, usize)>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ts_classes: BTreeMap<String, TsClass>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ts_literal_methods: BTreeMap<String, BTreeMap<String, bool>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub impls: Vec<Impl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<Impl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub externs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_use: Vec<String>,
    #[serde(default, skip_serializing_if = "lean")]
    pub mentions: Option<BTreeSet<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub function_aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unbuilt: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usings: Option<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub misparsed: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub package: String,
    #[serde(default, skip_serializing_if = "lean")]
    pub annotated: Vec<Annotated>,
    #[serde(default, skip_serializing_if = "lean")]
    pub bound: Vec<Bound>,
    #[serde(default, skip_serializing_if = "lean")]
    pub assigned: Vec<Bound>,
    #[serde(default, skip_serializing_if = "lean")]
    pub mentioned: Vec<Mentioned>,
    #[serde(default, skip_serializing_if = "lean")]
    pub private_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reflects: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anonymous: Vec<String>,
    #[serde(default, skip_serializing_if = "lean")]
    pub first_results: Vec<FirstResult>,
    #[serde(default, skip_serializing_if = "lean")]
    pub field_writes: Vec<FieldWrites>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_writes: Option<Vec<MemberWrite>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ts_record_writes: Vec<MemberWrite>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ts_record_writes_incomplete: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<Range<usize>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred: Vec<Range<usize>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repeated: Vec<Range<usize>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partial: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expect: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actual: Vec<String>,
    #[serde(skip)]
    pub unit: String,
    #[serde(skip)]
    pub unit_references: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub testable: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confined: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sealed: Vec<String>,
    #[serde(skip)]
    pub internals: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub setting: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub macro_rules: BTreeMap<String, Option<Vec<String>>>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub invokes: BTreeSet<String>,
}

pub fn setting(base: &str, extra: &str) -> String {
    match extra {
        "" => base.to_string(),
        extra => format!("{base}\n{extra}"),
    }
}

pub fn base(setting: &str) -> &str {
    setting.split_once('\n').map_or(setting, |(base, _)| base)
}

pub fn extra(setting: &str) -> &str {
    setting.split_once('\n').map_or("", |(_, extra)| extra)
}

#[derive(Serialize, Deserialize)]
pub struct Mentioned {
    pub scope: String,
    pub name: String,
    pub count: u32,
    #[serde(default)]
    pub read: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Bound {
    pub scope: String,
    pub within: String,
    pub name: String,
    pub value: Value,
}

#[derive(Serialize, Deserialize)]
pub struct FieldWrites {
    pub name: String,
    pub values: Vec<(String, Value)>,
}

#[derive(Serialize, Deserialize)]
pub struct FirstResult {
    pub scope: String,
    pub name: String,
    pub call: Range<usize>,
}

pub(crate) fn outermost(root: Node, kinds: &[&str]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if kinds.contains(&node.kind()) {
            out.push(node.byte_range());
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    out
}

pub(crate) fn mentioned<'l>(
    root: Node,
    src: &[u8],
    defs: &[Def],
    locals: impl Iterator<Item = (&'l str, &'l str)>,
    read: impl Fn(Node) -> bool,
) -> Vec<Mentioned> {
    let locals: BTreeSet<(&str, &str)> = locals.collect();
    let mut written: HashMap<&str, [Vec<usize>; 2]> = locals
        .iter()
        .map(|&(_, name)| (name, Default::default()))
        .collect();
    if written.is_empty() {
        return Vec::new();
    }
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "identifier" {
            let Some(text) = node.utf8_text(src).ok().filter(|t| !t.contains('\\')) else {
                return Vec::new();
            };
            if let Some(at) = written.get_mut(text.trim_start_matches('@')) {
                at[usize::from(read(node))].push(node.start_byte());
            }
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    (written.values_mut().flatten()).for_each(|at| at.sort_unstable());
    let mut functions: HashMap<&str, Vec<&Range<usize>>> = HashMap::new();
    for def in defs {
        if matches!(def.kind, Kind::Function | Kind::Method | Kind::Inline) {
            functions.entry(&def.name).or_default().push(&def.range);
        }
    }
    (locals.into_iter())
        .filter_map(|(scope, name)| {
            let ranges = functions.get(scope)?;
            let [count, read] = written[name].each_ref().map(|at| {
                let count: usize = (ranges.iter())
                    .map(|r| {
                        at.partition_point(|&s| s < r.end) - at.partition_point(|&s| s < r.start)
                    })
                    .sum();
                count.try_into().unwrap_or(u32::MAX)
            });
            Some(Mentioned {
                scope: scope.into(),
                name: name.into(),
                count,
                read,
            })
        })
        .collect()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MemberWrite {
    pub scope: String,
    pub object: String,
    pub field: Option<String>,
    pub range: Range<usize>,
}

impl MemberWrite {
    pub(crate) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.scope.len()
            + self.object.len()
            + self.field.as_ref().map_or(0, String::len)
    }
}

impl Facts {
    pub(crate) fn member_write(&mut self, write: MemberWrite, spent: &mut usize) {
        *spent = spent.saturating_add(write.bytes());
        if *spent > MAX_BYTES {
            self.member_writes = None;
            self.budget.get_or_insert(MAX_BYTES);
        } else if let Some(writes) = &mut self.member_writes {
            writes.push(write);
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Through {
    pub tried: bool,
    pub awaited: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Annotated {
    pub on: Marked,
    pub annotation: Call,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Marked {
    Field {
        class: String,
        name: String,
        is_static: bool,
    },
    Param(String, usize),
}

/// A block whose definitions are members of a type: those named `scope.<member>`. An inherent
#[derive(Serialize, Deserialize)]
pub struct Impl {
    pub scope: String,
    pub ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
    pub within: String,
}

#[derive(Serialize, Deserialize)]
pub struct Receiver {
    pub scope: String,
    pub name: String,
    pub class: Class,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TsLocal {
    pub block: Range<usize>,
    pub declared: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<usize>,
    #[serde(default, skip_serializing_if = "lean_or_absent")]
    pub constant: Option<usize>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct TsClass {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implements: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub initialized: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub constructed: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, TsFieldParameter>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub records: BTreeMap<String, (usize, Value)>,
}

#[derive(Serialize, Deserialize)]
pub struct TsFieldParameter {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip: Option<SlashStrip>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Class {
    Enclosing,
    TsGlobalValue,
    TsGlobalType(String),
    TsGeneric(String),
    Named(String),
    Type(String),
    Returned(String),
    Called(String),
    Result(usize),
    Member(String),
    Imported,
    Unknown,
}

#[derive(Serialize, Deserialize)]
pub struct Import {
    pub local: String,
    pub source: String,
    pub name: Imported,
}

#[derive(Serialize, Deserialize)]
pub enum Imported {
    Qualified,
    Name(String),
    Member {
        name: String,
        module: String,
    },
    Namespace,
    All,
    Path(String),
    Glob {
        path: String,
        inside: bool,
        public: bool,
    },
}

#[derive(Serialize, Deserialize)]
pub enum Export {
    Local {
        exported: String,
        local: String,
    },
    From {
        exported: String,
        source: String,
        name: Imported,
    },
    All {
        source: String,
    },
}

pub(crate) fn loaded(facts: &Facts) -> BTreeSet<&str> {
    let exported = facts.exports.iter().filter_map(|e| match e {
        Export::From { source, .. } | Export::All { source } => Some(source.as_str()),
        Export::Local { .. } => None,
    });
    (facts.imports.iter().map(|i| i.source.as_str()))
        .chain(exported)
        .chain(facts.loads.iter().map(String::as_str))
        .collect()
}

#[derive(Default, Serialize, Deserialize)]
pub enum Star {
    #[default]
    ExceptDefault,
    Public,
    Only(Vec<String>),
    Computed,
}

impl Star {
    pub fn takes(&self, name: &str) -> bool {
        match self {
            Star::ExceptDefault => name != "default",
            Star::Public | Star::Computed => !name.starts_with('_'),
            Star::Only(names) => names.iter().any(|n| n == name),
        }
    }

    pub fn taker(&self) -> Taker<'_> {
        let only = match self {
            Star::Only(names) => names.iter().map(String::as_str).collect(),
            _ => HashSet::new(),
        };
        Taker { star: self, only }
    }

    pub fn is_public_set(&self) -> bool {
        !matches!(self, Star::ExceptDefault)
    }
}

pub struct Taker<'a> {
    star: &'a Star,
    only: HashSet<&'a str>,
}

impl Taker<'_> {
    pub fn takes(&self, name: &str) -> bool {
        match self.star {
            Star::Only(_) => self.only.contains(name),
            star => star.takes(name),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Function,
    Class,
    Method,
    Inline,
    Module,
}

#[derive(Serialize, Deserialize)]
pub struct Def {
    pub name: String,
    pub kind: Kind,
    pub range: Range<usize>,
    pub params: Vec<Param>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decorators: Vec<String>,
    #[serde(default, skip_serializing_if = "lean")]
    pub return_values: Vec<Value>,
    #[serde(default, skip_serializing_if = "lean")]
    pub reassigned_params: Vec<String>,
    #[serde(default, skip_serializing_if = "lean")]
    pub terminal_return: bool,
    #[serde(default, skip_serializing_if = "lean")]
    pub single_return: bool,
    #[serde(default, skip_serializing_if = "lean")]
    pub returns: Option<Ty>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_params: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Class>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unwrapped: Option<Class>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Class>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suspends: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub asynchronous: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hash: String,
}

impl Def {
    pub fn new(name: impl Into<String>, kind: Kind, range: Range<usize>) -> Self {
        Def {
            name: name.into(),
            kind,
            range,
            params: Vec::new(),
            bases: Vec::new(),
            decorators: Vec::new(),
            return_values: Vec::new(),
            reassigned_params: Vec::new(),
            terminal_return: false,
            single_return: false,
            returns: None,
            type_params: Vec::new(),
            result: None,
            unwrapped: None,
            output: None,
            suspends: false,
            asynchronous: false,
            private: false,
            hash: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    #[serde(default)]
    pub binds: Binds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "lean")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
    #[serde(default, skip_serializing_if = "Closure::takes")]
    pub closure: Closure,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Closure {
    #[default]
    Takes,
    Refuses,
    Unknown,
    Named(String),
}

impl Closure {
    fn takes(&self) -> bool {
        *self == Closure::Takes
    }
}

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Binds {
    #[default]
    Any,
    Keyword,
    Rest,
    Optional,
    Instance,
    Class,
}

#[derive(Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "lean")]
    pub default: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Declaration {
    pub name: String,
    pub range: Range<usize>,
    pub hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Use {
    Jsx,
    Type,
    Value,
    Member,
    Extends,
    Reexport,
    Dynamic,
}

impl Use {
    const ALL: [Use; 7] = [
        Use::Jsx,
        Use::Type,
        Use::Value,
        Use::Member,
        Use::Extends,
        Use::Reexport,
        Use::Dynamic,
    ];

    pub fn names(id: &str) -> bool {
        let last = id.rsplit("::").next().unwrap_or(id);
        (Self::ALL.iter()).any(|k| {
            (last.strip_prefix('(')).is_some_and(|rest| {
                rest.strip_prefix(k.as_str())
                    .is_some_and(|r| r.starts_with(')'))
            })
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Use::Jsx => "jsx",
            Use::Type => "type",
            Use::Value => "value",
            Use::Member => "member",
            Use::Extends => "extends",
            Use::Reexport => "reexport",
            Use::Dynamic => "dynamic",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub scope: String,
    pub name: String,
    pub kind: Use,
    pub range: Range<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvField {
    pub class: String,
    pub field: String,
    pub var: String,
}

#[derive(Serialize, Deserialize)]
pub struct Const {
    pub name: String,
    #[serde(default = "unknown", skip_serializing_if = "lean")]
    pub value: Value,
    #[serde(default, skip_serializing_if = "lean")]
    pub ty: Option<Ty>,
    #[serde(default, skip_serializing_if = "lean")]
    pub scope: Option<String>,
}

fn unknown() -> Value {
    Value::Unknown
}

pub fn field_write(name: &str, class: Option<&str>) -> Const {
    Const {
        name: match class {
            Some(class) => format!(".{name}@{class}"),
            None => format!(".{name}"),
        },
        value: Value::Unknown,
        ty: None,
        scope: None,
    }
}

#[derive(Serialize, Deserialize)]
pub struct Call {
    pub scope: String,
    pub callee: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arity: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub constructs: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub awaited: bool,
    pub range: Range<usize>,
    #[serde(default, skip_serializing_if = "lean")]
    pub args: Vec<Value>,
    #[serde(default, skip_serializing_if = "lean")]
    pub kwargs: Vec<(String, Value)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorates: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_path: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unwraps: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub awaits: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "lean")]
    pub receiver: Option<Value>,
    #[serde(default, skip_serializing_if = "lean")]
    pub arg_types: Vec<Option<Ty>>,
    #[serde(default, skip_serializing_if = "lean")]
    pub uses: Vec<Vec<Step>>,
}

impl Call {
    pub fn keyword(&self, name: &str) -> Option<&Value> {
        self.kwargs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Field(String),
    Element,
    Call(String),
    /// The value is declared to have this type: by a cast, a type argument of the method called
    Typed(Ty),
    Argument(Vec<Ty>),
    Return,
}

// ponytail: real types nest a handful of levels; the limit keeps a crafted chain of `extends`
// or aliases, or a type nested thousands of levels, from costing the stack.
pub const TYPE_DEPTH: usize = 32;

// ponytail: a flat 1 MiB; a streaming reader that keeps only the dependency tables lifts it.
pub(crate) const MAX_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ty {
    Object { members: Vec<Member>, open: bool },
    Named(String),
    Intersection(Vec<Ty>),
    Array(Box<Ty>),
    Schema(String),
    Param(String),
    Unknown,
}

impl Ty {
    pub fn has_param(&self) -> bool {
        match self {
            Ty::Param(_) => true,
            Ty::Array(of) => of.has_param(),
            Ty::Intersection(parts) => parts.iter().any(Ty::has_param),
            Ty::Object { members, .. } => members.iter().any(|m| m.ty.has_param()),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub optional: bool,
    pub ty: Ty,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Value {
    Await(Box<Value>),
    Str(Vec<Part>),
    Object(Vec<(String, Value)>),
    Ref(String),
    Undefined,
    Call(Invocation),
    Member(String, String),
    Schema(String),
    New(String, Vec<Value>),
    List(Vec<Value>),
    Type(Ty),
    Scalar(Option<u32>),
    Function(String),
    External(Vec<String>),
    Unknown,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Invocation {
    pub callee: String,
    pub receiver: Option<Box<Value>>,
    pub args: Vec<Value>,
    pub kwargs: Vec<(String, Value)>,
    pub range: Range<usize>,
}

pub struct Depends;

pub fn property<'v>(
    properties: &'v [(String, Value)],
    key: &str,
) -> std::result::Result<Option<&'v Value>, Depends> {
    static UNKNOWN: Value = Value::Unknown;
    for (k, value) in properties.iter().rev() {
        match (k.as_str(), value) {
            (k, value) if k == key => return Ok(Some(value)),
            ("[]", Value::List(pair)) => match pair.as_slice() {
                [Value::Str(parts), value] => match literal(parts) {
                    Some(name) if name == key => return Ok(Some(value)),
                    Some(_) => {}
                    None if parts.iter().any(|p| matches!(p, Part::Ref(_))) => return Err(Depends),
                    None => return Ok(Some(&UNKNOWN)),
                },
                [Value::Ref(_), _] => return Err(Depends),
                _ => return Ok(Some(&UNKNOWN)),
            },
            ("...", Value::Object(inner)) => {
                if let Some(found) = property(inner, key)? {
                    return Ok(Some(found));
                }
            }
            ("...", Value::Undefined) => {}
            ("...", Value::Ref(_)) => return Err(Depends),
            ("...", _) => return Ok(Some(&UNKNOWN)),
            _ => {}
        }
    }
    Ok(None)
}

pub fn literal(parts: &[Part]) -> Option<String> {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum SlashStrip {
    One,
    All,
}

impl SlashStrip {
    pub(crate) fn apply(self, text: &str) -> &str {
        match self {
            Self::One => text.strip_suffix('/').unwrap_or(text),
            Self::All => text.trim_end_matches('/'),
        }
    }

    pub(crate) fn wrap(self, mut parts: Vec<Part>) -> Vec<Part> {
        self.finish(&mut parts, 0);
        parts
    }

    fn finish(self, parts: &mut Vec<Part>, start: usize) {
        while parts.len() > start {
            let Some(Part::Text(last)) = parts.last_mut() else {
                parts.insert(start, Part::StripStart);
                parts.push(Part::StripEnd(self));
                return;
            };
            let len = self.apply(last).len();
            let done = len > 0 || (self == Self::One && !last.is_empty());
            last.truncate(len);
            if done {
                return;
            }
            parts.pop();
        }
    }
}

pub(crate) fn fold_strips(parts: Vec<Part>) -> Vec<Part> {
    if !parts.iter().any(|p| matches!(p, Part::StripStart)) {
        return parts;
    }
    let mut out = Vec::new();
    let mut starts = Vec::new();
    for part in parts {
        match part {
            Part::StripStart => starts.push(out.len()),
            Part::StripEnd(strip) if let Some(start) = starts.pop() => {
                strip.finish(&mut out, start)
            }
            other => out.push(other),
        }
    }
    out
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Part {
    StripStart,
    StripEnd(SlashStrip),
    RecordStart {
        at: usize,
        field: String,
        property: String,
    },
    RecordEnd,
    RecordValues {
        key: Option<String>,
        values: Vec<Vec<Part>>,
    },
    Text(String),
    Ref(String),
    Env(String),
    EnvKey {
        name: String,
        /// When the parser only knows the spelling of the reader, the binding to verify.
        reader: Option<String>,
    },
    Invoke {
        callee: String,
        range: Range<usize>,
        awaited: bool,
    },
    EnvOr(String, Vec<Part>),
    Read {
        #[serde(with = "as_str")]
        scope: std::sync::Arc<str>,
        callee: String,
        key: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        parameter: bool,
        then: Vec<String>,
    },
    Limit(Limit),
    QuerySuffix,
    Hole,
}

impl Part {
    pub(crate) fn invoke(callee: String, range: Range<usize>, awaited: bool) -> Self {
        Self::Invoke {
            callee,
            range,
            awaited,
        }
    }

    pub(crate) fn env_key(value: &Value) -> Option<Self> {
        match value {
            Value::Str(parts) => literal(parts)
                .filter(|name| !name.is_empty())
                .map(Self::Env),
            Value::Ref(name) => Some(Self::EnvKey {
                name: name.clone(),
                reader: None,
            }),
            _ => None,
        }
    }

    pub(crate) fn parameter(&self) -> Option<&str> {
        match self {
            Self::Ref(name)
            | Self::EnvKey { name, .. }
            | Self::RecordValues {
                key: Some(name), ..
            } => Some(name),
            Self::Read {
                key,
                parameter: true,
                ..
            } => Some(key),
            _ => None,
        }
    }

    pub fn unread(value: &Value) -> Part {
        match value {
            Value::Call(call) => Self::invoke(call.callee.clone(), call.range.clone(), false),
            Value::Await(inner) if let Value::Call(call) = inner.as_ref() => {
                Self::invoke(call.callee.clone(), call.range.clone(), true)
            }
            Value::Await(inner) => Part::unread(inner),
            _ => Part::Hole,
        }
    }
}

pub(crate) fn query_alternative(left: Value, right: Value) -> Value {
    if matches!((&left, &right), (Value::Str(a), Value::Str(b)) if query_only(a) && query_only(b)) {
        Value::Str(vec![Part::QuerySuffix])
    } else {
        Value::Unknown
    }
}

fn first_text(parts: &[Part]) -> Option<&str> {
    match parts
        .iter()
        .find(|p| !matches!(p, Part::Text(t) if t.is_empty()))
    {
        Some(Part::Text(t)) => Some(t),
        None => Some(""),
        _ => None,
    }
}

fn query_only(parts: &[Part]) -> bool {
    matches!(parts, [Part::QuerySuffix])
        || first_text(parts).is_some_and(|text| text.is_empty() || text.starts_with('?'))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryWrite {
    Append,
    Replace,
    Other,
}

impl QueryWrite {
    pub(crate) fn of(value: &Value, name: &str, operator: &str) -> Self {
        let Value::Str(parts) = value else {
            return Self::Other;
        };
        let append = match (operator, parts.as_slice()) {
            ("+=", parts) => Some(parts),
            ("=", [Part::Ref(target), tail @ ..]) if target == name => Some(tail),
            _ => None,
        };
        if append
            .and_then(first_text)
            .is_some_and(|text| text.starts_with('?'))
        {
            Self::Append
        } else if operator == "=" && query_only(parts) {
            Self::Replace
        } else {
            Self::Other
        }
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Other, _) | (_, Self::Other) => Self::Other,
            (Self::Replace, _) | (_, Self::Replace) => Self::Replace,
            _ => Self::Append,
        }
    }

    pub(crate) fn keep(self, value: &mut Value) -> bool {
        let Value::Str(parts) = value else {
            return false;
        };
        match self {
            Self::Append => parts.push(Part::QuerySuffix),
            Self::Replace if query_only(parts) => *parts = vec![Part::QuerySuffix],
            _ => return false,
        }
        true
    }
}

/// Why value flow stops at a part of a string (`Part::Limit`), which a consumer whose URL starts
/// there names (#801).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Limit {
    /// The return value of a call Meada does not evaluate: its callee as written (`c.baseV2`).
    Returned(String),
    /// A parameter no call site binds: its name.
    Parameter(String),
    /// The base of a client Meada did not read.
    ClientBase,
    /// Any other part Meada could not read (a `Hole` or an undecided `Read`); only a consumer's
    /// lead names it, never a string.
    Other,
}

impl Limit {
    pub fn describe(&self) -> String {
        match self {
            Limit::Returned(callee) => format!("base from the return value of {callee}"),
            Limit::Parameter(name) => format!("base from the parameter {name}"),
            Limit::ClientBase => "base of a client Meada did not read".into(),
            Limit::Other => "base Meada could not read".into(),
        }
    }
}

fn bare_calls(facts: &mut Facts) {
    let locals: HashSet<&str> = facts.locals.iter().map(String::as_str).collect();
    let consts: HashSet<&str> = facts.consts.iter().map(|c| c.name.as_str()).collect();
    let properties: HashSet<(&str, &str)> = (facts.receivers.iter())
        .map(|r| (r.scope.as_str(), r.name.as_str()))
        .collect();
    let params: HashMap<&str, Vec<&str>> = (facts.defs.iter())
        .map(|d| {
            (
                d.name.as_str(),
                d.params.iter().map(|p| p.name.as_str()).collect(),
            )
        })
        .collect();
    let bound = |scope: &str, name: &str| {
        let mut at = Some(scope);
        while let Some(s) = at {
            let qualified = crate::qualify(s, name);
            if locals.contains(qualified.as_str())
                || consts.contains(qualified.as_str())
                || properties.contains(&(s, name))
                || params.get(s).is_some_and(|p| p.contains(&name))
            {
                return true;
            }
            at = (!s.is_empty()).then(|| crate::parent(s));
        }
        false
    };
    let bare: Vec<bool> = (facts.calls.iter())
        .map(|c| {
            c.decorates.is_none()
                && !c.callee.contains('.')
                && !c.callee.starts_with('(')
                && !bound(&c.scope, &c.callee)
        })
        .collect();
    for (call, bare) in facts.calls.iter_mut().zip(bare) {
        call.by_path = bare;
    }
}

pub const IMPLICIT_HEAD: &str = "(implicit)";

fn wildcard(source: &str, inside: bool, facts: &mut Facts) {
    let alias = format!("@glob{}", facts.imports.len());
    facts.imports.push(Import {
        local: alias.clone(),
        source: source.to_string(),
        name: Imported::Namespace,
    });
    facts.imports.push(Import {
        local: "*".into(),
        source: String::new(),
        name: Imported::Glob {
            path: alias,
            inside,
            public: false,
        },
    });
}

pub(crate) fn named<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    let mut cursor = node.walk();
    (node.named_children(&mut cursor))
        .filter(|n| !n.is_extra() || n.is_error())
        .collect::<Vec<_>>()
        .into_iter()
}

fn children<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect::<Vec<_>>().into_iter()
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.byte_range()]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn a_use_id_is_told_from_a_call_id_by_its_last_segment() {
        use super::Use;
        for id in [
            "r::a.ts::(value)X#0",
            "r::a.ts::f::(member)tokens.gap#1",
            "r::a.ts::(dynamic)default#0",
        ] {
            assert!(Use::names(id), "{id}");
        }
        for id in [
            "r::a.ts::(import)#0",
            "r::a.ts::f::g#0",
            "r::a.ts::(values)X#0",
            "r::a.ts::(value::x#0",
            "r::(value)x::f::g#0",
        ] {
            assert!(!Use::names(id), "{id}");
        }
    }

    fn positions(pack: &dyn super::LanguagePack, path: &str, src: &str) -> Vec<String> {
        let facts = pack.parse(path, src.as_bytes()).unwrap();
        let defs = facts.defs.iter().map(|d| {
            let params = serde_json::to_string(&d.params).unwrap();
            format!("def {} {params}", d.name)
        });
        let calls = facts.calls.iter().map(|c| {
            let labels = serde_json::to_string(&c.labels).unwrap();
            let (args, kwargs) = (c.args.len(), c.kwargs.len());
            format!("call {} {:?} {labels} {args} {kwargs}", c.callee, c.arity)
        });
        defs.chain(calls).collect()
    }

    #[test]
    fn a_comment_in_a_parameter_or_argument_list_takes_no_position() {
        use super::LanguagePack;
        let cases: [(&str, &dyn LanguagePack, &str); 8] = [
            (
                "x.go",
                &super::go::Go,
                "package x\nfunc f(# a int, # b int) {}\nfunc g() { f(# 1, # 2) }\n",
            ),
            (
                "x.rs",
                &super::rust::Rust,
                "fn f(# a: i32, # b: i32) {}\nfn g() { f(# 1, # 2) }\n",
            ),
            (
                "X.java",
                &super::java::Java,
                "class X { void f(# int a, # int b) {} void g() { f(# 1, # 2); } }\n",
            ),
            (
                "x.kt",
                &super::kotlin::Kotlin,
                "fun f(# a: Int, # b: Int) {}\nfun g() { f(# 1, # 2) }\n",
            ),
            (
                "x.swift",
                &super::swift::Swift,
                "func f(# a: Int, # b: Int) {}\nfunc g() { f(# a: 1, # b: 2) }\n",
            ),
            (
                "x.py",
                &super::python::Python,
                "def f(# a, # b): pass\ndef g(): f(# 1, # 2)\n",
            ),
            (
                "x.ts",
                &super::ts::TsJs,
                "function f(# a, # b) {}\nfunction g() { f(# 1, # 2) }\n",
            ),
            (
                "X.cs",
                &super::csharp::CSharp,
                "class X { void F(# int a, # int b) {} void G() { F(# 1, # 2); } }\n",
            ),
        ];
        let comments = ["// first\n", "/* middle */"];
        let python = ["# first\n", ""];
        for (path, pack, template) in cases {
            let [first, middle] = if path.ends_with(".py") {
                python
            } else {
                comments
            };
            let mut with = String::new();
            for (i, part) in template.split('#').enumerate() {
                with.push_str(part);
                if i + 1 < template.split('#').count() {
                    with.push_str(if part.ends_with('(') { first } else { middle });
                }
            }
            let without = template.replace('#', "");
            assert_eq!(
                positions(pack, path, &with),
                positions(pack, path, &without),
                "{path}"
            );
        }
    }

    const TABLES: &[(&str, &str)] = &[
        ("csharp.rs:TRIMS", "runtime"),
        ("go.rs:PREDECLARED", "syntax"),
        ("go/build.rs:ARCHITECTURES", "runtime"),
        ("go/build.rs:SYSTEMS", "runtime"),
        ("go/values.rs:METHODS", "exception"),
        ("java.rs:REFLECTIVE", "runtime"),
        ("kotlin.rs:CLOSEABLE", "runtime"),
        ("kotlin.rs:COMPARABLE", "runtime"),
        ("kotlin.rs:DECLARATION_KEYWORDS", "syntax"),
        ("kotlin.rs:DEFAULT_IMPORTS", "runtime"),
        ("kotlin.rs:FIELD_TARGETS", "syntax"),
        ("kotlin.rs:PARAM_TARGETS", "syntax"),
        ("kotlin.rs:STRING_METHODS", "runtime"),
        ("python.rs:STRING_METHODS", "runtime"),
        ("rust.rs:ROUTE_ATTRIBUTES", "exception"),
        ("rust.rs:VERBS", "exception"),
        ("rust/builders.rs:READING_MACROS", "runtime"),
        ("rust/builders.rs:READS", "runtime"),
        ("rust/crates.rs:KINDS", "syntax"),
        ("rust/values.rs:ENV_METHODS", "runtime"),
        ("rust/values.rs:FORMATS", "runtime"),
        ("rust/values.rs:KEEPS", "runtime"),
        ("rust/values.rs:KEEPS_NAMES", "runtime"),
        ("rust/values.rs:METHOD_CRATES", "exception"),
        ("swift.rs:VALUE_TYPES", "runtime"),
        ("swift/values.rs:STRING_METHODS", "runtime"),
        ("ts.rs:BODIES", "syntax"),
        ("ts.rs:CONFIG_READS", "exception"),
        ("ts.rs:EXTENSIONS", "syntax"),
        ("ts.rs:FUNCTIONS", "syntax"),
        ("ts.rs:STRING_METHODS", "runtime"),
        ("ts/types.rs:INFERS", "exception"),
        ("ts/types.rs:RESPONSES", "exception"),
        ("ts/uses.rs:ELEMENT_METHODS", "runtime"),
    ];

    fn tables(dir: &Path, root: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                tables(&path, root, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                for line in std::fs::read_to_string(&path).unwrap().lines() {
                    let item = line.trim_start().trim_start_matches("pub(crate) ");
                    let item = item.strip_prefix("pub ").unwrap_or(item);
                    if let Some(rest) = item.strip_prefix("const ").or(item.strip_prefix("static "))
                        && let Some((id, ty)) = rest.split_once(':')
                        && (ty.trim_start().starts_with("&[&str]")
                            || ty.trim_start().starts_with("[&str;"))
                    {
                        found.push(format!("{name}:{id}"));
                    }
                }
            }
        }
    }

    #[test]
    fn every_str_table_in_a_pack_is_listed_with_its_class() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lang");
        let mut found = Vec::new();
        tables(&root, &root, &mut found);
        found.sort();
        let listed: Vec<_> = TABLES.iter().map(|(table, _)| table.to_string()).collect();
        assert_eq!(found, listed, "list a new table in TABLES, with its class");
        for (table, class) in TABLES {
            assert!(
                ["syntax", "runtime", "exception"].contains(class),
                "{table}: {class}"
            );
        }
    }
}
