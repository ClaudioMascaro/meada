//! The language adapter (ADR 0007): the seam between a language pack, which reads one file
//! at a time, and the shared resolution in `resolve`, which connects calls to definitions.
//! A pack never touches the index or other files.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::resolve::Code;

pub mod ts;

pub trait LanguagePack {
    /// The file extensions the pack parses, without the dot.
    fn extensions(&self) -> &'static [&'static str];

    /// What one file defines, imports, exports and calls. `path` is repository-relative.
    fn parse(&self, path: &str, src: &[u8]) -> Result<Facts>;

    /// Repository-relative files that configure how imports name files, such as
    /// `tsconfig.json`. The shared side reads those present and hands them to `modules`.
    fn config_files(&self) -> &'static [&'static str];

    /// How imports name files in one repository, built from its config files as
    /// `(path, contents)`, in `config_files` order.
    fn modules(&self, configs: &[(&str, &[u8])]) -> Result<Modules>;

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
const PACKS: &[&dyn LanguagePack] = &[&ts::TsJs];

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
    pub exports: Vec<Export>,
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
    /// The program entry points of the file (spec #31), set by `parse`, which sees the path and
    /// the source: the qualified names (`Def::name`) of the definitions that start a program,
    /// such as `func main` in package `main`, `fn main` in `src/main.rs`. Where the entry point
    /// is top-level code rather than a function (`if __name__ == "__main__":`), `parse` yields a
    /// definition for that code, with the calls it makes, and names it here. Each becomes an
    /// entry point of kind `program`, labelled by its qualified name. A name with no symbol is
    /// dropped without a word: each pack's conformance golden is what catches a wrong one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
}

/// A file-level binding to what another module exports: `import { name as local } from
/// 'source'`.
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
    /// The whole module.
    Namespace,
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
    /// Every name `source` exports, except `default`.
    All { source: String },
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
    /// A function's declared return type, inside the `Promise` and `NextResponse` it is wrapped
    /// in (`Promise<z.infer<typeof S>>` gives that schema), when it lists any fields. Not kept in
    /// the index (see `Call::args`).
    #[serde(skip)]
    pub returns: Option<Ty>,
    /// The names of a function's type parameters in order (`get<T>`). Not kept in the index.
    #[serde(skip)]
    pub type_params: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    /// The default value, as written. Not kept in the index (see `Call::args`).
    #[serde(skip)]
    pub default: Option<Value>,
    /// The names an object pattern binds, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
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
    Object(Vec<(String, Value)>),
    /// A name: a constant, a parameter, a definition.
    Ref(String),
    /// `undefined`, or an argument the call leaves out.
    Undefined,
    /// The result of calling the normalized callee, such as `express.Router`.
    Call(Invocation),
    /// A property of a named value, such as `parsed.data` (spec #23).
    Member(String, String),
    /// A value declared to have the type a schema infers, such as `x as z.infer<typeof S>`
    /// (spec #23): the name of the schema.
    Schema(String),
    /// The result of `new` on the normalized callee, such as `Kafka` for `new Kafka(...)`, with
    /// the arguments.
    New(String, Vec<Value>),
    /// An array literal's elements.
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
/// the callee is no member.
#[derive(Clone)]
pub struct Invocation {
    pub callee: String,
    pub receiver: Option<Box<Value>>,
    pub args: Vec<Value>,
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

#[derive(Clone)]
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
