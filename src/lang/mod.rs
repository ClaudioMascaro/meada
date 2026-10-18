//! The language adapter (ADR 0007): the seam between a language pack, which reads one file
//! at a time, and the shared resolution in `resolve`, which connects calls to definitions.
//! A pack never touches the index or other files.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use crate::Result;
use crate::resolve::Code;

pub mod python;
pub mod rust;
pub mod ts;

pub trait LanguagePack {
    /// The file extensions the pack parses, without the dot.
    fn extensions(&self) -> &'static [&'static str];

    /// What one file defines, imports, exports and calls. `path` is repository-relative.
    fn parse(&self, path: &str, src: &[u8]) -> Result<Facts>;

    /// Repository-relative files that configure how imports name files, such as
    /// `tsconfig.json`. The shared side reads those present and hands them to `modules` and
    /// `members`. A name starting with `**/` names every file of that name the walk finds, at
    /// any depth, in path order (`**/Cargo.toml`: a Cargo workspace's members).
    fn config_files(&self) -> &'static [&'static str];

    /// How imports name files in one repository, built from its config files as
    /// `(path, contents)`, in `config_files` order, and from the facts of the pack's files
    /// (the files `members` keeps): a Rust `mod` declaration names a file by where the
    /// declaring file sits in its crate's module tree.
    fn modules(&self, configs: &[(&str, &[u8])], files: &BTreeMap<&str, &Facts>)
    -> Result<Modules>;

    /// The files the language builds, among those it parsed, when it says which: a Rust file
    /// is part of a crate only when a `mod` declaration reaches it from a crate root. `None`
    /// keeps every file. A file left out has no symbol or call, and resolves nothing.
    fn members(
        &self,
        _configs: &[(&str, &[u8])],
        _files: &BTreeMap<&str, &Facts>,
    ) -> Option<BTreeSet<String>> {
        None
    }

    /// The files whose exports (the `exports` table) another package can import, when the
    /// language says which: a Rust file's `pub` items are reachable from outside its crate
    /// only through a library root and `pub mod` declarations, and a binary exports nothing.
    /// `None` keeps every file.
    fn published(
        &self,
        _configs: &[(&str, &[u8])],
        _files: &BTreeMap<&str, &Facts>,
    ) -> Option<BTreeSet<String>> {
        None
    }

    /// The program entry points only the whole repository tells, as `(file, qualified name)`
    /// among the files `members` kept: a Rust `fn main` starts a program only in a binary
    /// target's root, which `parse` of one file cannot know. `Facts::programs` still applies.
    fn programs(
        &self,
        _configs: &[(&str, &[u8])],
        _files: &BTreeMap<&str, &Facts>,
    ) -> Vec<(String, String)> {
        Vec::new()
    }

    /// A keyword naming the instance of the enclosing class anywhere in it, such as `this`. A
    /// language that names it as a parameter (`self`) writes it in `Facts::receivers` instead.
    fn receiver(&self) -> Option<&'static str> {
        None
    }

    /// A keyword naming the base of the enclosing class, such as `super`: what its members
    /// are is not resolved.
    fn base_receiver(&self) -> Option<&'static str> {
        None
    }

    /// Whether a method call that resolves to nothing in the repository hands back the value
    /// it was called on, as a builder does (`Router::new().route(..)` in Rust): `Code::origins`
    /// then follows the call's `receiver`. A language whose methods return anything stays
    /// with the call as the origin.
    fn fluent(&self) -> bool {
        false
    }

    /// Names every file of the language may call without binding them, which contract kinds
    /// read, such as `fetch`.
    fn globals(&self) -> &'static [&'static str] {
        &[]
    }

    /// The producer and consumer calls of the message libraries the pack knows (spec #20),
    /// read from its files' facts through `code`: a call counts only when its receiver traces
    /// back to the library's package (`Code::origin`), and its channel name is evaluated like
    /// a URL, client wrappers included (`Code::expand`). The channel contract kind turns them
    /// into contracts; a pack never touches the index.
    fn messages(&self, _code: &Code) -> Vec<Message> {
        Vec::new()
    }
}

/// A call that produces to or consumes from a channel: a queue or topic of one broker kind.
pub struct Message {
    /// `kafka`, `amqp`, `sqs`, `nats`, `bullmq` or `redis`.
    pub broker: &'static str,
    /// The channel name as evaluated, kept as it is; `None` when it cannot be evaluated (an
    /// unresolved side, never linked).
    pub name: Option<String>,
    /// The call site: the producing or subscribing call, or the call of a client wrapper that
    /// leads to it, as its file and position in `Facts::calls`.
    pub path: String,
    pub call: usize,
    pub role: Role,
}

pub enum Role {
    /// The provider side of the channel.
    Producer,
    /// A publish to a named exchange (`name` is its routing key): the message reaches the
    /// queues bound to the exchange whose pattern matches, wherever the binding is declared.
    Publish { exchange: String },
    /// A binding of the queue `name` to an exchange by a pattern. It is neither side of a
    /// channel; the channel kind keeps it for the links.
    Binding { exchange: String, pattern: String },
    /// The consumer side, with the function that handles what it receives.
    Consumer(Handler),
}

/// Where a consumer's messages are handled.
#[derive(Clone)]
pub enum Handler {
    /// A definition by qualified name; an empty name is the file's top level. A name whose
    /// last segments are inline definitions no kind names stands for the definition that
    /// encloses them.
    Def { path: String, name: String },
    /// An anonymous function written in place: the `Kind::Inline` definition `def` of `path`,
    /// which the channel kind names after the channels it handles.
    Inline { path: String, def: String },
}

/// The repository-relative paths an import of a specifier (second argument) from a file
/// (first argument) may name, most likely first. The shared side takes the first one that
/// the pack parses.
pub type Modules = Box<dyn Fn(&str, &str) -> Vec<String>>;

/// Every pack Meada ships, in a fixed order.
const PACKS: &[&dyn LanguagePack] = &[&ts::TsJs, &python::Python, &rust::Rust];

/// The index of the pack that parses `path`, if any.
pub fn pack_for(path: &str) -> Option<usize> {
    let (_, extension) = path.rsplit_once('.')?;
    PACKS
        .iter()
        .position(|p| p.extensions().contains(&extension))
}

pub fn pack(index: usize) -> &'static dyn LanguagePack {
    PACKS[index]
}

/// Every pack, in `PACKS` order.
#[cfg(test)]
pub fn packs() -> impl Iterator<Item = &'static dyn LanguagePack> {
    PACKS.iter().copied()
}

/// One file's facts, each list in source order. The index keeps them, so a refresh resolves
/// unchanged files without parsing them again.
#[derive(Default, Serialize, Deserialize)]
pub struct Facts {
    pub defs: Vec<Def>,
    pub imports: Vec<Import>,
    /// Everything an explicit import can name: `exports` lists every binding the file makes
    /// available, and `star` narrows what a star import takes from them.
    pub exports: Vec<Export>,
    pub star: Star,
    pub calls: Vec<Call>,
    /// `const` bindings that hold no function or class, such as `const USERS = "/users"` or
    /// `const app = express()`. Their values are not kept in the index (see `Call::args`).
    pub consts: Vec<Const>,
    /// Every other name a binding makes, qualified like `Const::name` by the definition whose
    /// body or parameters hold it (a function that is no definition lends its names to the
    /// one around it, at the top level the whole file): `let` and `var`, destructuring, `catch`
    /// and `for ... of` bindings, and each parameter name `Def::params` does not hold (a nested,
    /// array or rest pattern, or any parameter of a function that is no definition). Each hides
    /// the same spelling in its whole scope (`Resolver::lexical`), whatever block it sits in: an
    /// outer name, and a definition, constant or parameter of its own scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locals: Vec<String>,
    /// Names bound in a scope whose class the syntax may tell, one per scope and name: what a
    /// method call on the name reaches. Each also hides an outer binding of its name.
    pub receivers: Vec<Receiver>,
    /// The program entry points of the file (spec #31), set by `parse`, which sees the path and
    /// the source: the qualified names (`Def::name`) of the definitions that start a program,
    /// such as `func main` in package `main`, `fn main` in `src/main.rs`. Where the entry point
    /// is top-level code rather than a function (`if __name__ == "__main__":`), `parse` yields a
    /// definition for that code, with the calls it makes, and names it here. Each becomes an
    /// entry point of kind `program`, labelled by its qualified name. A name with no symbol is
    /// dropped without a word: each pack's conformance golden is what catches a wrong one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
    /// The blocks that add definitions to a type written elsewhere, such as Rust's `impl`:
    /// the shared side looks a member of the type up in each block too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub impls: Vec<Impl>,
    /// Names a path of the file begins with that nothing in it binds: the crates outside the
    /// file (Rust's extern prelude). They resolve after every import and glob: to another crate
    /// of the repository by its name, else to something outside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub externs: Vec<String>,
    /// The `exports` names only the package itself sees (Rust's items that are not `pub`):
    /// they resolve for the package's own files but are not in the exports table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private: Vec<String>,
}

/// A block whose definitions are members of a type: those named `scope.<member>`. An inherent
/// Rust `impl Store` is the scope `Store`; a trait's `impl Persist for Store` is `Store.Persist`,
/// whose members come after the inherent ones, and whose trait lends its default bodies.
#[derive(Serialize, Deserialize)]
pub struct Impl {
    pub scope: String,
    /// The type, as a callee-like name resolved from `within`.
    pub ty: String,
    /// The trait a trait's block implements, resolved the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
    /// The scope enclosing the block: an inline module, or the top level.
    pub within: String,
}

/// A name bound in `scope`: a local, a parameter, or, when `scope` is a class, an attribute of
/// its instances. The shared side follows a method call on it to the class it holds.
#[derive(Serialize, Deserialize)]
pub struct Receiver {
    pub scope: String,
    pub name: String,
    pub class: Class,
}

/// What the syntax says a name holds. It is read as written: the shared side decides whether
/// the name it gives is a class, and no type is inferred beyond that.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum Class {
    /// An instance of the class whose body encloses the scope, or that class: `self`, `cls`.
    Enclosing,
    /// What this callee-like name (`Circle`, `shapes.Circle`), resolved from the scope, gives
    /// an instance of: a constructor call or an annotation. A function's result is unknown.
    Named(String),
    /// What this path (`Store`, or `store::Store` as `store.Store`), written as a type, names:
    /// an instance of that class. Its head is read among types and modules, so a local or a
    /// parameter of the same name does not hide it (Rust's `store: store::Store`).
    Type(String),
    /// What the function this callee-like name gives, called in the scope, returns: the class
    /// its `Def::result` names (Rust's `let s = Store::new()`). Something outside the
    /// repository stays outside; a function with no written result is unknown.
    Returned(String),
    /// Bound by an import in the scope: what the file's imports bind the name to.
    Imported,
    /// Bound otherwise, so an outer binding of the name is hidden.
    Unknown,
}

/// A file-level binding to what another module exports: `import { name as local } from
/// 'source'`. Several imports may bind one `local`: the latest in source order decides. A
/// `local` may be a dotted name (`a.b.c`), which a call whose callee starts with it uses as its
/// head.
#[derive(Serialize, Deserialize)]
pub struct Import {
    pub local: String,
    pub source: String,
    pub name: Imported,
}

#[derive(Serialize, Deserialize)]
pub enum Imported {
    /// One exported name; a default import is the name `default`.
    Name(String),
    /// A name the module exports or, when it exports none (or the module is not a file of
    /// the repository), the module `module`: `from p import m` names `m` of `p` or the
    /// submodule `p.m`. `module` is a specifier, as `Import::source` is.
    Member { name: String, module: String },
    /// The whole module.
    Namespace,
    /// Every name the module's `star` takes, as `from source import *` binds them (`local` is
    /// unused). It binds a name only when the module takes it, and then like an import of
    /// that name written there.
    All,
    /// What this callee-like name (`crate.a.f`, `inner.helper`) gives, resolved from the scope
    /// that holds `local` (Rust's `use a::f as local`). `source` is unused. Only the scope
    /// of the name's own `local` sees it, and a binding of the name in a scope inside hides it.
    Path(String),
    /// Every name the module this callee-like name gives takes, as a `use path::*` binds them
    /// (`local` is the scope's `*`, `qualify(scope, "*")`; `source` is unused). `inside` is
    /// whether the scope is within that module, which then gives its private items too.
    /// `public` is whether the glob re-exports (`pub use`).
    Glob {
        path: String,
        inside: bool,
        public: bool,
    },
}

#[derive(Serialize, Deserialize)]
pub enum Export {
    /// `exported` names the file-level binding `local`: a definition or an import.
    Local { exported: String, local: String },
    /// `exported` names what `source` exports as `name`: a re-export.
    From {
        exported: String,
        source: String,
        name: Imported,
    },
    /// Every name `source` exports that its `star` lets through.
    All { source: String },
}

/// Which of a file's exports a star import (`export * from`, `from m import *`) takes.
#[derive(Default, Serialize, Deserialize)]
pub enum Star {
    /// Every one except `default`.
    #[default]
    ExceptDefault,
    /// Those not starting with `_`.
    Public,
    /// Only these names: a literal `__all__`.
    Only(Vec<String>),
}

impl Star {
    /// Whether a star import takes the export `name`.
    pub fn takes(&self, name: &str) -> bool {
        match self {
            Star::ExceptDefault => name != "default",
            Star::Public => !name.starts_with('_'),
            Star::Only(names) => names.iter().any(|n| n == name),
        }
    }

    /// Whether the pack names a public set: every export but those a star import leaves out is
    /// one. `ExceptDefault` names none, since a default export is as public as any.
    pub fn is_public_set(&self) -> bool {
        !matches!(self, Star::ExceptDefault)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Function,
    Class,
    Method,
    /// An anonymous function passed as an argument, such as a route handler or a callback.
    /// The last segment of its name is `@<byte offset>`, which no identifier can be: the
    /// shared side names it when a contract kind says what it is (an inline route handler)
    /// and folds it into its enclosing definition otherwise.
    Inline,
    /// A module written in place (Rust's `mod inner { }`): its definitions are named under it
    /// (`inner.helper`), and a function of it is matched by name like a top-level one.
    Module,
}

/// A definition. `name` is qualified by its enclosing definitions: `outer.inner`,
/// `Class.method`.
#[derive(Serialize, Deserialize)]
pub struct Def {
    pub name: String,
    pub kind: Kind,
    pub range: Range<usize>,
    /// A function's parameters in order; a destructured one has an empty name and lists the
    /// names it binds in `fields`.
    pub params: Vec<Param>,
    /// A class's bases as written, in order, as callee-like names resolved from the scope
    /// enclosing the class.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<String>,
    /// The decorators written as a bare name (`@require_GET`, `@http.require_safe`), as
    /// callee-like names resolved from the scope enclosing the definition. A decorator call
    /// is a `Call` that names the definition in `Call::decorates`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decorators: Vec<String>,
    /// What a function returns, one value per `return`. Not kept in the index (see
    /// `Call::args`).
    #[serde(skip)]
    pub return_values: Vec<Value>,
    /// A function's declared return type, inside the `Promise` and `NextResponse` it is wrapped
    /// in (`Promise<z.infer<typeof S>>` gives that schema), when it lists any fields. Not kept in
    /// the index (see `Call::args`).
    #[serde(skip)]
    pub returns: Option<Ty>,
    /// The names of a function's type parameters in order (`get<T>`). Not kept in the index.
    #[serde(skip)]
    pub type_params: Vec<String>,
    /// The class a function's written return type names (`Self`, `Store`, a trait of `impl
    /// Trait`), as a callee-like name resolved from the function's own scope: what a call to
    /// it gives (`Class::Returned`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Class>,
}

#[derive(Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    /// Which argument of a call binds it.
    #[serde(default)]
    pub binds: Binds,
    /// The default value, as written. Not kept in the index (see `Call::args`).
    #[serde(skip)]
    pub default: Option<Value>,
    /// The names an object pattern binds, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
}

/// Which argument of a call binds a parameter. A positional argument binds the parameters that
/// take one in order; a keyword argument binds the parameter of its name.
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Binds {
    /// A positional or a keyword argument.
    #[default]
    Any,
    /// Only a keyword argument: a Python parameter after `*` or `*args`.
    Keyword,
    /// No single argument: Python's `*args` and `**kwargs`.
    Rest,
    /// The receiver of a method (`self`): a call on an instance binds it, so its positional
    /// arguments start at the next parameter; a call on the class passes it first.
    Instance,
    /// The receiver of a class method (`cls`): the call binds it, on the class or on an
    /// instance.
    Class,
}

/// A name bound by a destructured object parameter: `{ key: name = default }`.
#[derive(Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    /// The property of the argument it reads.
    pub key: String,
    /// The default value, as written. Not kept in the index.
    #[serde(skip)]
    pub default: Option<Value>,
}

/// A `const` binding, qualified by its enclosing definitions like `Def::name`.
#[derive(Serialize, Deserialize)]
pub struct Const {
    pub name: String,
    /// Not kept in the index (see `Call::args`).
    #[serde(skip, default = "unknown")]
    pub value: Value,
    /// The type the constant is declared to have, by an annotation or a cast (spec #23), when
    /// it lists fields: the value as written may lack what the type has, an index signature.
    /// Not kept in the index, like `value`.
    #[serde(skip)]
    pub ty: Option<Ty>,
}

fn unknown() -> Value {
    Value::Unknown
}

/// A call site. `scope` is the qualified name of the enclosing definition, empty at the top
/// level; `callee` is the normalized call expression: `a.b.c` for identifier and member
/// chains, `(<node kind>)` for any other expression.
#[derive(Serialize, Deserialize)]
pub struct Call {
    pub scope: String,
    pub callee: String,
    pub range: Range<usize>,
    /// The arguments, as far as the call's own syntax tells: what contract kinds read URLs
    /// and options from. Not kept in the index: they hold string literals, so the index
    /// parses a file again when a contract needs them.
    #[serde(skip)]
    pub args: Vec<Value>,
    /// The keyword arguments, as written, after the positional ones. Not kept in the index.
    #[serde(skip)]
    pub kwargs: Vec<(String, Value)>,
    /// The qualified name of the definition a decorator call applies to: `@app.get("/")`
    /// written on `def index()` decorates `index`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorates: Option<String>,
    /// The callee is a path (Rust's `a::b::f()`), not a member chain: the name match a call
    /// that does not resolve falls back to takes functions as well as methods.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub by_path: bool,
    /// The value a method is called on (`Router::new()` in `Router::new().route(..)`), when
    /// the pack gives it; a pack whose `fluent` is true has it followed. Not kept in the
    /// index, like `args`.
    #[serde(skip)]
    pub receiver: Option<Value>,
    /// The type each argument is cast to (`body as Reply`), by position, when it lists fields
    /// (spec #23). Not kept in the index, like `args`.
    #[serde(skip)]
    pub arg_types: Vec<Option<Ty>>,
    /// Shape facts (spec #23), an optional part of a pack's output: what the function making
    /// the call does with its result, one chain of steps per use, such as `res.json()` then
    /// `.items`, `[0]` and `.id`. A pack that leaves it empty gives its consumers no shape.
    /// Not kept in the index, like `args`.
    #[serde(skip)]
    pub uses: Vec<Vec<Step>>,
}

impl Call {
    /// The keyword argument `name`.
    pub fn keyword(&self, name: &str) -> Option<&Value> {
        self.kwargs.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

/// One step of what code does with a value (spec #23): the value read through a chain of
/// member and element accesses and method calls, then maybe returned. A chain ends where the
/// pack cannot follow the value (passed to a function, stored, rebound).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// `.name`, `?.name` or `["name"]`.
    Field(String),
    /// An element: `[0]`, an index with a literal number.
    Element,
    /// A call of the method `name` on the value: `.json()`.
    Call(String),
    /// The value is declared to have this type: by a cast, a type argument of the method called
    /// or an annotation on the variable that receives it. A chain ends here, but the reads that
    /// follow the cast go on in a chain of their own.
    Typed(Ty),
    /// The type arguments the call itself is given (`get<Row>(path)`), in order. They type the
    /// result only if the function called says so (a cast of its result to a type parameter, or
    /// a return type that is one): the caller cannot tell. A chain of its own, at the start.
    Argument(Vec<Ty>),
    /// The value is returned by the function making the call.
    Return,
}

/// How deep a pack reads a type, and the shared side follows one (spec #23): the nesting of
/// the type as written, and the names it follows on the way. A part deeper than that adds no
/// fields.
// ponytail: real types nest a handful of levels; the limit keeps a crafted chain of `extends`
// or aliases, or a type nested thousands of levels, from costing the stack.
pub const TYPE_DEPTH: usize = 32;

/// A type as written (spec #23), as far as drift reads it: the fields it lists. A form that
/// lists none is `Unknown`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    /// `{ a: A; b?: B }`, or an interface body. `open` when it has an index signature.
    Object { members: Vec<Member>, open: bool },
    /// A name: an interface, an alias, a class or an import; the shared side resolves it.
    /// Type arguments are not substituted.
    Named(String),
    /// `A & B`, and an interface with the types it extends.
    Intersection(Vec<Ty>),
    /// `T[]` and `Array<T>`.
    Array(Box<Ty>),
    /// `z.infer<typeof S>`: the name of the schema.
    Schema(String),
    /// A type parameter of the function around the use (`as T` in `get<T>`): the caller's type
    /// argument stands for it. One of an interface or alias adds no fields.
    Param(String),
    /// Any other form: a union, a primitive, a mapped type.
    Unknown,
}

impl Ty {
    /// Whether a type parameter is written in the type.
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

/// A property of an object type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub optional: bool,
    pub ty: Ty,
}

/// An expression as written. The pack does not follow names: the shared side folds
/// `Ref`s into the constants and arguments they name (`resolve`), and after that only
/// parameters whose arguments are not known remain as `Ref`s.
#[derive(Clone)]
pub enum Value {
    /// A string: literals, template literals and `+` concatenation, folded into parts.
    Str(Vec<Part>),
    /// An object literal's properties; a spread `...x` is the key `...` with the value of `x`.
    /// A Python dict literal is one too: an entry whose key is no string literal is the key
    /// `[]` with the list of its key and value.
    Object(Vec<(String, Value)>),
    /// A name: a constant, a parameter, a definition.
    Ref(String),
    /// `undefined`, or an argument the call leaves out.
    Undefined,
    /// The result of a call, such as `express.Router()`.
    Call(Invocation),
    /// A property of a named value, such as `parsed.data` (spec #23).
    Member(String, String),
    /// A value declared to have the type a schema infers, such as `x as z.infer<typeof S>`
    /// (spec #23): the name of the schema.
    Schema(String),
    /// The result of `new` on the normalized callee, such as `Kafka` for `new Kafka(...)`, with
    /// the arguments.
    New(String, Vec<Value>),
    /// An array, list or tuple literal's items.
    List(Vec<Value>),
    /// An `interface` or `type` declaration (spec #23), held by a `Const` of the name it
    /// declares: the type namespace shares the name of a `const` of the same spelling.
    Type(Ty),
    /// A number or a boolean literal (spec #23): a scalar. A number written as a plain integer
    /// carries it (an HTTP status).
    Scalar(Option<u32>),
    /// An anonymous function written in place, by the qualified name of its `Kind::Inline`
    /// definition.
    Function(String),
    /// Anything else.
    Unknown,
}

/// A call as written (spec #23): `receiver` is the expression a method is called on (`z` in
/// `z.object(..)`, the call before `.optional()` in `z.string().optional()`), left out when
/// the callee is no member. `range` is the range of that call among the file's calls, which
/// the shared side resolves to follow the value it returns.
#[derive(Clone)]
pub struct Invocation {
    pub callee: String,
    pub receiver: Option<Box<Value>>,
    pub args: Vec<Value>,
    pub range: Range<usize>,
}

/// What a reader of evaluated values needs depends on a parameter of the enclosing function:
/// the function is a client wrapper (see `Code::expand`).
pub struct Depends;

/// The value of the last property `key` of an object, as JavaScript reads it, through spreads:
/// `None` when no property sets it, `Unknown` when a spread of something unknown may.
pub fn property<'v>(
    properties: &'v [(String, Value)],
    key: &str,
) -> std::result::Result<Option<&'v Value>, Depends> {
    static UNKNOWN: Value = Value::Unknown;
    for (k, value) in properties.iter().rev() {
        match (k.as_str(), value) {
            (k, value) if k == key => return Ok(Some(value)),
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

/// The text of `parts` when every part is literal.
pub fn literal(parts: &[Part]) -> Option<String> {
    parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// A channel name as evaluated: `None` when it depends on a parameter, `Some(None)` when it is
/// not a literal string (an environment read, a runtime value, a regular expression).
pub fn channel_name(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Ref(_) => None,
        Value::Str(parts) if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        Value::Str(parts) => Some(literal(parts)),
        _ => Some(None),
    }
}

/// The SQS queue the URL `url` names: the last segment of the URL, whatever precedes it (an
/// account and a region are often dynamic). `None` when it depends on a parameter, `Some(None)`
/// when it cannot be read.
pub fn queue_name(url: Option<&Value>) -> Option<Option<String>> {
    let parts = match url {
        Some(Value::Str(parts)) => parts,
        Some(Value::Ref(_)) => return None,
        _ => return Some(None),
    };
    match parts.last() {
        Some(Part::Text(tail)) if tail.contains('/') => {
            let queue = tail.rsplit('/').next().filter(|q| !q.is_empty());
            Some(queue.map(String::from))
        }
        _ if parts.iter().any(|p| matches!(p, Part::Ref(_))) => None,
        _ => Some(None),
    }
}

#[derive(Clone, PartialEq)]
pub enum Part {
    Text(String),
    /// A name inside a string, folded like `Value::Ref`.
    Ref(String),
    /// A read of one environment variable, such as `process.env.X`, through `||`/`??`
    /// fallbacks and string method calls. Its value is never known.
    Env(String),
    /// Any other expression inside a string.
    Hole,
}

// Tree-sitter helpers every pack uses.

fn named<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
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
