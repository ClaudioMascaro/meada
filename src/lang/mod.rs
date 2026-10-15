//! The language adapter (ADR 0007): the seam between a language pack, which reads one file
//! at a time, and the shared resolution in `resolve`, which connects calls to definitions.
//! A pack never touches the index or other files.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::Result;

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
    Call(String),
    /// An anonymous function written in place, by the qualified name of its `Kind::Inline`
    /// definition.
    Function(String),
    /// Anything else.
    Unknown,
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
