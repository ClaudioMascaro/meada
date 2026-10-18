//! The values of the Rust pack: what a call is given and what it is called on, what a `let` or a
//! `const` binds, and what a function returns, as far as the syntax tells. Contract kinds read
//! them through the shared value flow (`Code::origins`). A value is built once the whole file
//! is read, since a path in it needs the file's `use` declarations (`normalize`).
//!
//! A call inside a value is a stub: its callee and its span, which the shared side matches to
//! the file's calls. Its own arguments and receiver are on that call (`Call::args`,
//! `Call::receiver`), so a chain of a thousand calls costs a thousand values, not a million.
//!
//! Strings fold the way a URL is written: literals, `+`, `format!` and `concat!` (a format
//! string and comma-separated arguments, `{base}` captures included), `String::from` and the
//! conversions that keep the text (`to_string`, `as_str`), a `String` a function builds
//! (`builders.rs`) and the `Method::GET` constants of the HTTP crates.

use std::collections::HashMap;

use tree_sitter::Node;

use super::builders::{BUILDER_DEPTH, BUILDER_SIZE, BUILDER_WORK, Builder, size};
use super::{DEFINITION_DEPTH, Heads, Path, Scan, field, levels, member, normalize, segments};
use crate::lang::{Invocation, Part, Value, children, named, text};
use crate::qualify;

/// How many levels of wrappers (`(x)`, `&x`, `x?`, `a + b`) and arrays a value is read through;
/// deeper is `Unknown`.
// ponytail: real code wraps a value a few levels; a crafted file must not recurse without end.
pub(super) const VALUE_DEPTH: usize = 64;

/// The HTTP methods the `Method` constants of the HTTP crates name, and those crates.
const VERBS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE",
];
const METHOD_CRATES: &[&str] = &["reqwest", "http", "hyper", "axum", "actix_web"];

/// The methods that keep the text of a string, and those that keep it when called on a name
/// (a conversion of a name may be to another type: `into`, `clone`).
const KEEPS: &[&str] = &["to_string", "to_owned", "into", "clone", "as_str", "as_ref"];
const KEEPS_NAMES: &[&str] = &["to_string", "to_owned", "as_str"];

/// The methods an environment read goes through: those that keep the text of a string
/// (`trim_end_matches`) and those that unwrap its `Result` (`unwrap_or(fallback)` among them).
const ENV_METHODS: &[&str] = &[
    "trim",
    "trim_start",
    "trim_end",
    "trim_matches",
    "trim_start_matches",
    "trim_end_matches",
    "to_string",
    "to_owned",
    "as_str",
    "into",
    "clone",
    "unwrap",
    "unwrap_or",
    "unwrap_or_else",
    "unwrap_or_default",
    "expect",
];

/// The macros whose arguments are a format string and the values it prints.
const FORMATS: &[&str] = &["format", "concat"];

/// How many arguments and placeholders a `format!` or `concat!` is read with, and how big its
/// text may be (`builders::size`); past any of them the value is unknown.
// ponytail: a URL is built from a few arguments; a crafted file with 300k `{u}` captures must
// not copy a builder for each.
pub(super) const FORMAT_ARGS: usize = 64;
pub(super) const FORMAT_PLACEHOLDERS: usize = 256;
const FORMAT_SIZE: usize = BUILDER_SIZE;

/// Where a value is written: the scope, the module written in place and whether `self::` and
/// `super::` can be read there, as a `Path` records them.
pub(super) struct Where {
    pub scope: String,
    pub module: String,
    pub pure: bool,
}

/// The builders of a file by scope and name (`builders.rs`).
pub(super) type Builders<'t> = HashMap<(String, String), Builder<'t>>;

pub(super) struct Reader<'a, 't> {
    src: &'a [u8],
    scan: &'a Scan,
    heads: &'a mut Heads,
    builders: &'a Builders<'t>,
    /// The parts each step of a builder (0 is what it began as) adds, read once.
    folded: HashMap<(String, String, usize), Vec<Part>>,
    /// What each builder holds after 0, 1, 2.. of its pushes, folded as far as a read needed.
    prefixes: HashMap<(String, String), Vec<Vec<Part>>>,
    /// How many builders are being read within one another.
    nest: usize,
    /// What the builders' folds and reads have cost, in `size` units (`BUILDER_WORK`).
    work: usize,
}

impl<'a, 't> Reader<'a, 't> {
    pub fn new(
        src: &'a [u8],
        scan: &'a Scan,
        heads: &'a mut Heads,
        builders: &'a Builders<'t>,
    ) -> Self {
        Reader {
            src,
            scan,
            heads,
            builders,
            folded: HashMap::new(),
            prefixes: HashMap::new(),
            nest: 0,
            work: 0,
        }
    }

    /// The value of the expression `node`.
    pub fn value(&mut self, at: &Where, node: Node<'t>) -> Value {
        self.value_at(at, node, 0)
    }

    fn value_at(&mut self, at: &Where, node: Node<'t>, depth: usize) -> Value {
        let mut node = node;
        for level in depth..VALUE_DEPTH {
            match node.kind() {
                "parenthesized_expression" | "await_expression" | "try_expression" => {
                    match named(node).next() {
                        Some(inner) => node = inner,
                        None => return Value::Unknown,
                    }
                }
                "reference_expression" | "type_cast_expression" => match field(node, "value") {
                    Some(inner) => node = inner,
                    None => return Value::Unknown,
                },
                "generic_function" => match field(node, "function") {
                    Some(inner) => node = inner,
                    None => return Value::Unknown,
                },
                "array_expression" => {
                    let items = named(node).map(|n| self.value_at(at, n, level + 1));
                    return Value::List(items.collect());
                }
                _ => return self.leaf(at, node, level),
            }
        }
        Value::Unknown
    }

    fn leaf(&mut self, at: &Where, node: Node<'t>, level: usize) -> Value {
        let src = self.src;
        match node.kind() {
            "string_literal" | "raw_string_literal" => string(node, src),
            "identifier" => {
                let name = text(node, src);
                match self.built(at, name, node.start_byte()) {
                    Some(parts) => Value::Str(parts),
                    None => Value::Ref(name.to_string()),
                }
            }
            "self" => Value::Ref(text(node, src).to_string()),
            "scoped_identifier" => match self.http_method(at, node) {
                Some(method) => method,
                None => match self.path(at, node) {
                    Some(name) => Value::Ref(name),
                    None => Value::Unknown,
                },
            },
            "field_expression" => match member(node, src) {
                name if name.starts_with('(') => Value::Unknown,
                name => Value::Ref(name),
            },
            "call_expression" => self.call_value(at, node, level),
            "binary_expression" => self.sum(at, node, level),
            "macro_invocation" => self.macro_value(at, node),
            _ => Value::Unknown,
        }
    }

    /// The normalized name of the path `node` (`crate.a.f`).
    fn path(&mut self, at: &Where, node: Node) -> Option<String> {
        let path = self.path_of(at, segments(node, self.src)?);
        normalize(&path, self.scan, self.heads, false)
    }

    fn path_of(&self, at: &Where, segments: Vec<String>) -> Path {
        Path {
            scope: at.scope.clone(),
            module: at.module.clone(),
            pure: at.pure,
            segments,
            binds: None,
        }
    }

    /// The normalized callee of a call to `function`, as `parse` spells it.
    fn callee(&mut self, at: &Where, function: Node) -> String {
        let function = match function.kind() {
            "generic_function" => field(function, "function").unwrap_or(function),
            _ => function,
        };
        match function.kind() {
            "identifier" => text(function, self.src).to_string(),
            "scoped_identifier" => self
                .path(at, function)
                .unwrap_or_else(|| "(scoped_identifier)".into()),
            "field_expression" => member(function, self.src),
            kind => format!("({kind})"),
        }
    }

    /// A call: the text of a string it keeps (`String::from(x)`, `"x".to_string()`), else the
    /// stub the shared side matches to the file's calls.
    fn call_value(&mut self, at: &Where, node: Node<'t>, level: usize) -> Value {
        let Some(function) = field(node, "function") else {
            return Value::Unknown;
        };
        if let Some(read) = self.env_read(at, node, 0) {
            return Value::Str(vec![read]);
        }
        let args: Vec<Node> = (field(node, "arguments").into_iter())
            .flat_map(named)
            .filter(|a| !is_comment(*a))
            .collect();
        match (function.kind(), args.as_slice()) {
            ("scoped_identifier", _) => {
                let segments = segments(function, self.src).unwrap_or_default();
                let owned = !self
                    .scan
                    .defines(&self.path_of(at, segments.clone()), "String");
                match (segments.as_slice(), args.as_slice()) {
                    ([s, f], [arg]) if s == "String" && f == "from" && owned => {
                        return match self.value_at(at, *arg, level + 1) {
                            value @ (Value::Str(_) | Value::Ref(_)) => value,
                            _ => Value::Unknown,
                        };
                    }
                    ([s, f], []) if s == "String" && f == "new" && owned => {
                        return Value::Str(Vec::new());
                    }
                    ([s, f], [_]) if s == "String" && f == "with_capacity" && owned => {
                        return Value::Str(Vec::new());
                    }
                    _ => {}
                }
            }
            ("field_expression", []) => {
                let method = field(function, "field").map_or("", |f| text(f, self.src));
                if let Some(receiver) = field(function, "value").filter(|_| KEEPS.contains(&method))
                {
                    match self.value_at(at, receiver, level + 1) {
                        value @ Value::Str(_) => return value,
                        value @ Value::Ref(_) if KEEPS_NAMES.contains(&method) => return value,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Value::Call(Invocation {
            callee: self.callee(at, function),
            receiver: None,
            args: Vec::new(),
            range: node.byte_range(),
        })
    }

    /// `a + b`: the parts of the two sides, where at least one is a string; a side that is not
    /// is a hole.
    fn sum(&mut self, at: &Where, node: Node<'t>, level: usize) -> Value {
        let plus = field(node, "operator").is_some_and(|o| text(o, self.src) == "+");
        let (Some(left), Some(right), true) = (field(node, "left"), field(node, "right"), plus)
        else {
            return Value::Unknown;
        };
        // Two names may add numbers: one side has to be a string.
        let sides = [left, right].map(|n| self.value_at(at, n, level + 1));
        if !sides.iter().any(|v| matches!(v, Value::Str(_))) {
            return Value::Unknown;
        }
        let parts = sides.into_iter().flat_map(|side| match side {
            Value::Str(parts) => parts,
            Value::Ref(name) => vec![Part::Ref(name)],
            _ => vec![Part::Hole],
        });
        Value::Str(merge(parts))
    }

    /// `Method::GET`, as a text, when the path names a constant of the `Method` of the HTTP
    /// crates: through a `use` of the file, or written in full.
    fn http_method(&self, at: &Where, node: Node) -> Option<Value> {
        let segments = segments(node, self.src)?;
        let [.., kind, verb] = segments.as_slice() else {
            return None;
        };
        if kind != "Method" || !VERBS.contains(&verb.as_str()) {
            return None;
        }
        let full = self.spelled(at, &segments)?;
        METHOD_CRATES
            .contains(full.first()?)
            .then(|| Value::Str(vec![Part::Text(verb.clone())]))
    }

    /// The path `segments` names with the `use` of its head applied: the head is the one
    /// `use` that imports it, or is a crate written in full.
    fn spelled<'s>(&self, at: &Where, segments: &'s [String]) -> Option<Vec<&'s str>>
    where
        'a: 's,
    {
        let head = segments.first()?;
        match self
            .scan
            .defines(&self.path_of(at, segments.to_vec()), head)
        {
            true => {
                let target = self.target(at, head)?;
                Some(
                    target
                        .iter()
                        .chain(&segments[1..])
                        .map(String::as_str)
                        .collect(),
                )
            }
            false => Some(segments.iter().map(String::as_str).collect()),
        }
    }

    /// The path the one `use` of `name` in scope at `at` imports.
    fn target(&self, at: &Where, name: &str) -> Option<&'a Vec<String>> {
        let mut scope = at.scope.as_str();
        loop {
            let qualified = qualify(scope, name);
            if self.scan.defs.contains(&qualified) {
                return None;
            }
            if let Some(target) = self.scan.targets.get(&qualified) {
                return (self.scan.uses.get(&qualified) == Some(&1)).then_some(target);
            }
            if scope.len() <= at.module.len() {
                return None;
            }
            scope = crate::parent(scope);
        }
    }

    /// The environment read the expression `node` is, if it is one: `std::env::var("X")`,
    /// `env::var("X")` after `use std::env`, `dotenvy::var("X")` or `env!("X")`, through
    /// `unwrap`, `unwrap_or(..)`, `expect`, `?` and the string methods that keep the text. A
    /// second read in an argument (a fallback) makes it a hole: which of the two is the base
    /// is unknown. The value of the variable is never read.
    // ponytail: an argument is searched for the names `env`, `var` and `dotenvy`: one that only
    // looks like a read makes a hole, which is never wrong, only less.
    fn env_read(&mut self, at: &Where, node: Node<'t>, depth: usize) -> Option<Part> {
        if depth >= VALUE_DEPTH {
            return None;
        }
        match node.kind() {
            "parenthesized_expression" | "try_expression" => {
                self.env_read(at, named(node).next()?, depth + 1)
            }
            "macro_invocation" => {
                let name = self.macro_name(field(node, "macro")?)?;
                if name == "env" && !self.scan.macros.contains(&name) {
                    return self.env_macro(node);
                }
                None
            }
            "call_expression" => {
                let function = field(node, "function")?;
                if function.kind() != "field_expression" {
                    return self.env_var(at, node, function);
                }
                let method = text(field(function, "field")?, self.src);
                if !ENV_METHODS.contains(&method) {
                    return None;
                }
                let read = self.env_read(at, field(function, "value")?, depth + 1)?;
                let mut args = field(node, "arguments").into_iter().flat_map(named);
                Some(match args.any(|a| mentions_env(a, self.src)) {
                    true => Part::Hole,
                    false => read,
                })
            }
            _ => None,
        }
    }

    /// `std::env::var("X")` and `dotenvy::var("X")`, written in full or through a `use`.
    fn env_var(&self, at: &Where, call: Node, function: Node) -> Option<Part> {
        let segments = segments(function, self.src)?;
        if !matches!(
            self.spelled(at, &segments)?.as_slice(),
            ["std", "env", "var"] | ["dotenvy", "var"]
        ) {
            return None;
        }
        let mut args = field(call, "arguments").into_iter().flat_map(named);
        match (args.next(), args.next()) {
            (Some(name), None) => self.literal(name).map(Part::Env),
            _ => None,
        }
    }

    /// `env!("X")`: the variable the macro names.
    fn env_macro(&self, node: Node) -> Option<Part> {
        let tree = named(node).find(|c| c.kind() == "token_tree")?;
        self.env_name(tree).map(Part::Env)
    }

    /// The name in the first argument of the token tree of `env!("X")` or `env!("X", "msg")`.
    fn env_name(&self, tree: Node) -> Option<String> {
        match arguments(tree)?.first()?.as_slice() {
            [name] => self.literal(*name),
            _ => None,
        }
    }

    /// The text of the string literal `node`, if that is what it is.
    fn literal(&self, node: Node) -> Option<String> {
        match string(node, self.src) {
            Value::Str(parts) => match <[Part; 1]>::try_from(parts) {
                Ok([Part::Text(text)]) => Some(text),
                _ => None,
            },
            _ => None,
        }
    }

    /// `format!` and `concat!`: a format string and the arguments that fill it. An argument
    /// that is a literal or a plain name folds; any other is a hole, and so is a name with a
    /// format spec. A macro of another name, or one the file defines, is unknown.
    fn macro_value(&mut self, at: &Where, node: Node<'t>) -> Value {
        let Some(name) = field(node, "macro").and_then(|m| self.macro_name(m)) else {
            return Value::Unknown;
        };
        if name == "env" && !self.scan.macros.contains(&name) {
            return self
                .env_macro(node)
                .map_or(Value::Unknown, |e| Value::Str(vec![e]));
        }
        if !FORMATS.contains(&name.as_str()) || self.scan.macros.contains(&name) {
            return Value::Unknown;
        }
        let Some(args) = named(node)
            .find(|c| c.kind() == "token_tree")
            .and_then(arguments)
        else {
            return Value::Unknown;
        };
        let parts = match name.as_str() {
            "format" => self.format(at, node.start_byte(), &args),
            // `concat!` takes literals only: a name there is no string.
            _ if args.len() > FORMAT_ARGS => None,
            _ => {
                let parts = args.iter().flat_map(|a| match a.as_slice() {
                    [n] if n.kind() == "identifier" => vec![Part::Hole],
                    _ => self.argument(at, a),
                });
                let parts = merge(parts.collect::<Vec<_>>());
                (size(&parts) <= FORMAT_SIZE).then_some(parts)
            }
        };
        parts.map_or(Value::Unknown, Value::Str)
    }

    /// The macro `path` names: `format`, or `std::format` and its likes.
    fn macro_name(&self, path: Node) -> Option<String> {
        let name = match path.kind() {
            "identifier" => text(path, self.src).to_string(),
            "scoped_identifier" => {
                let segments = segments(path, self.src)?;
                match segments.as_slice() {
                    [krate, name] if matches!(krate.as_str(), "std" | "core" | "alloc") => {
                        name.clone()
                    }
                    _ => return None,
                }
            }
            _ => return None,
        };
        Some(name)
    }

    /// The parts of a format string filled with `args` (the first is the string).
    fn format(&mut self, at: &Where, pos: usize, args: &[Vec<Node<'t>>]) -> Option<Vec<Part>> {
        let ([template], rest) = (args.first()?.as_slice(), &args[1..]) else {
            return None;
        };
        let Value::Str(template) = (match template.kind() {
            "string_literal" | "raw_string_literal" => string(*template, self.src),
            _ => return None,
        }) else {
            return None;
        };
        let [Part::Text(template)] = template.as_slice() else {
            return None;
        };
        let mut positional: Vec<Vec<Part>> = Vec::new();
        let mut keyed: HashMap<String, Vec<Part>> = HashMap::new();
        if rest.len() > FORMAT_ARGS {
            return None;
        }
        for arg in rest {
            match arg.as_slice() {
                [name, eq, value @ ..] if name.kind() == "identifier" && eq.kind() == "=" => {
                    let parts = self.argument(at, value);
                    keyed.insert(text(*name, self.src).to_string(), parts);
                }
                _ if keyed.is_empty() => positional.push(self.argument(at, arg)),
                _ => return None,
            }
        }
        let (mut out, mut literal, mut next, mut holes) = (Vec::new(), String::new(), 0, 0);
        let mut rest = template.as_str();
        while let Some(c) = rest.chars().next() {
            if let Some(after) = rest.strip_prefix("{{").or_else(|| rest.strip_prefix("}}")) {
                literal.push(c);
                rest = after;
            } else if c == '{' {
                let end = rest.find('}')?;
                let (target, spec) = rest[1..end].split_once(':').unwrap_or((&rest[1..end], ""));
                // `{:.*}` and `{:1$}` take other arguments than the one named.
                if spec.contains(['*', '$']) {
                    return None;
                }
                let target = target.trim();
                let value = if target.is_empty() {
                    next += 1;
                    positional.get(next - 1)?.clone()
                } else if target.bytes().all(|b| b.is_ascii_digit()) {
                    positional.get(target.parse::<usize>().ok()?)?.clone()
                } else if let Some(value) = keyed.get(target) {
                    value.clone()
                } else if is_name(target) {
                    self.capture(at, target, pos)
                } else {
                    return None;
                };
                if !literal.is_empty() {
                    out.push(Part::Text(std::mem::take(&mut literal)));
                }
                // A spec (`{:?}`, `{x:>8}`) changes the text: whatever it prints is a hole.
                match spec.is_empty() {
                    true => out.extend(value),
                    false => out.push(Part::Hole),
                }
                holes += 1;
                if holes > FORMAT_PLACEHOLDERS || size(&out) > FORMAT_SIZE {
                    return None;
                }
                rest = &rest[end + 1..];
            } else if c == '}' {
                return None;
            } else {
                literal.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        out.push(Part::Text(literal));
        (size(&out) <= FORMAT_SIZE).then(|| merge(out))
    }

    /// The parts of a macro argument: a literal, a plain name, else a hole.
    fn argument(&mut self, at: &Where, nodes: &[Node<'t>]) -> Vec<Part> {
        // `env!("X")` is three tokens: the macro's name, `!` and its arguments.
        if let [name, bang, tree] = nodes
            && [name.kind(), bang.kind(), tree.kind()] == ["identifier", "!", "token_tree"]
            && text(*name, self.src) == "env"
            && !self.scan.macros.contains("env")
        {
            return vec![self.env_name(*tree).map_or(Part::Hole, Part::Env)];
        }
        let [node] = nodes else {
            return vec![Part::Hole];
        };
        let literal = |text: &str| vec![Part::Text(text.to_string())];
        match node.kind() {
            "string_literal" | "raw_string_literal" => match string(*node, self.src) {
                Value::Str(parts) => parts,
                _ => vec![Part::Hole],
            },
            // A suffix (`1u8`) or a radix (`0x10`) is not the digits it prints.
            "integer_literal" | "boolean_literal" => {
                let text = text(*node, self.src);
                match text.bytes().all(|b| b.is_ascii_digit()) || node.kind() == "boolean_literal" {
                    true => literal(text),
                    false => vec![Part::Hole],
                }
            }
            "identifier" => self.capture(at, text(*node, self.src), node.start_byte()),
            _ => vec![Part::Hole],
        }
    }

    /// The parts of the name `name`, read at `pos`: a builder's text, else the name.
    fn capture(&mut self, at: &Where, name: &str, pos: usize) -> Vec<Part> {
        match self.built(at, name, pos) {
            Some(parts) => parts,
            None => vec![Part::Ref(name.to_string())],
        }
    }

    /// The text the builder `name` holds at `pos`: what it began as and the pushes before `pos`.
    /// Each prefix of the pushes is folded once, from the one before it, and a read copies it.
    fn built(&mut self, at: &Where, name: &str, pos: usize) -> Option<Vec<Part>> {
        let builders = self.builders;
        if builders.is_empty() {
            return None;
        }
        // A closure written in an argument is a scope of its own, inside the function.
        let mut scope = at.scope.as_str();
        let (key, builder) = loop {
            let key = (scope.to_string(), name.to_string());
            if let Some(found) = builders.get_key_value(&key) {
                break found;
            }
            if !scope.rsplit('.').next().is_some_and(|s| s.starts_with('@')) {
                return None;
            }
            scope = crate::parent(scope);
        };
        if pos < builder.from || pos >= builder.to {
            return None;
        }
        if self.nest >= BUILDER_DEPTH || self.work > BUILDER_WORK {
            return Some(vec![Part::Hole]);
        }
        self.nest += 1;
        let place = Where {
            scope: key.0.clone(),
            module: at.module.clone(),
            pure: at.pure,
        };
        if !self.prefixes.contains_key(key) {
            let init = self.step(&place, key, 0, builder.init, false);
            self.prefixes.insert(key.clone(), vec![merge(init)]);
        }
        let n = builder.steps.partition_point(|p| p.end <= pos);
        while self.prefixes[key].len() <= n && self.work <= BUILDER_WORK {
            let k = self.prefixes[key].len();
            let push = &builder.steps[k - 1];
            let added = self.step(&place, key, k, push.arg, push.char);
            let mut next = self.prefixes[key][k - 1].clone();
            next.extend(added);
            let mut next = merge(next);
            if size(&next) > BUILDER_SIZE {
                next = vec![Part::Hole];
            }
            self.work += size(&next);
            if let Some(prefixes) = self.prefixes.get_mut(key) {
                prefixes.push(next);
            }
        }
        self.nest -= 1;
        let parts = (self.prefixes[key].get(n).cloned()).unwrap_or_else(|| vec![Part::Hole]);
        self.work += size(&parts);
        Some(parts)
    }

    /// The parts step `n` of the builder `key` adds, read once.
    fn step(
        &mut self,
        at: &Where,
        key: &(String, String),
        n: usize,
        node: Node<'t>,
        char: bool,
    ) -> Vec<Part> {
        let id = (key.0.clone(), key.1.clone(), n);
        if let Some(parts) = self.folded.get(&id) {
            return parts.clone();
        }
        let mut parts = match (char, self.value(at, node)) {
            (_, Value::Str(parts)) => parts,
            (false, Value::Ref(name)) => vec![Part::Ref(name)],
            (true, _) if node.kind() == "char_literal" => char_text(node, self.src),
            _ => vec![Part::Hole],
        };
        if size(&parts) > BUILDER_SIZE {
            parts = vec![Part::Hole];
        }
        self.work += size(&parts);
        self.folded.insert(id, parts.clone());
        parts
    }

    /// The arguments of the call `node` and the value it is called on. A closure written as an
    /// argument is the `Kind::Inline` definition `parse` made of it.
    pub fn call(&mut self, at: &Where, node: Node<'t>) -> (Vec<Value>, Option<Value>) {
        let function = field(node, "function").map(|f| match f.kind() {
            "generic_function" => field(f, "function").unwrap_or(f),
            _ => f,
        });
        let receiver = function
            .filter(|f| f.kind() == "field_expression")
            .and_then(|f| field(f, "value"))
            .map(|v| self.value(at, v));
        let deep = levels(&at.scope) >= DEFINITION_DEPTH;
        let args = field(node, "arguments").into_iter().flat_map(named);
        let args = args.filter(|a| !is_comment(*a));
        let args = args.map(|arg| match arg.kind() {
            "closure_expression" if !deep => Value::Function(inline(&at.scope, arg)),
            _ => self.value(at, arg),
        });
        (args.collect(), receiver)
    }

    /// What a function with this `body` returns: its tail expression and each `return`.
    pub fn returns(&mut self, at: &Where, body: Node<'t>) -> Vec<Value> {
        let mut out = Vec::new();
        let tail = named(body).filter(|n| !is_comment(*n)).last();
        out.extend(tail.map(|t| self.value(at, t)));
        let mut stack = vec![body];
        while let Some(node) = stack.pop() {
            match node.kind() {
                // Another function's `return` is not this one's, and a macro body is a token
                // tree.
                "function_item" | "closure_expression" | "macro_invocation" | "impl_item"
                | "mod_item" | "trait_item" => {}
                "return_expression" => {
                    let value = named(node).next();
                    out.push(value.map_or(Value::Undefined, |v| self.value(at, v)));
                }
                _ => stack.extend(named(node).collect::<Vec<_>>().into_iter().rev()),
            }
        }
        out
    }
}

/// Whether the expression `node` has a name that may read the environment (`env`, `var`,
/// `dotenvy`), in any depth, a macro's tokens included.
fn mentions_env(node: Node, src: &[u8]) -> bool {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if node.kind() == "identifier" && matches!(text(node, src), "env" | "var" | "dotenvy") {
            return true;
        }
        stack.extend(children(node));
    }
    false
}

fn is_comment(node: Node) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

/// Whether `text` is a plain name (a format string's `{base}`).
fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// The comma-separated arguments of a macro's token tree; `None` for an empty one between two
/// commas. A trailing comma adds none.
fn arguments(tree: Node) -> Option<Vec<Vec<Node>>> {
    let tokens: Vec<Node> = children(tree).collect();
    let inside = tokens.get(1..tokens.len().checked_sub(1)?)?;
    let mut args: Vec<Vec<Node>> = vec![Vec::new()];
    for token in inside {
        match token.kind() {
            "," => args.push(Vec::new()),
            "line_comment" | "block_comment" => {}
            _ => args.last_mut()?.push(*token),
        }
    }
    if args.last().is_some_and(Vec::is_empty) {
        args.pop();
    }
    (!args.iter().any(Vec::is_empty)).then_some(args)
}

/// `parts` with neighbouring texts joined.
fn merge(parts: impl IntoIterator<Item = Part>) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for part in parts {
        match (out.last_mut(), part) {
            (Some(Part::Text(last)), Part::Text(more)) => last.push_str(&more),
            (_, Part::Text(t)) if t.is_empty() => {}
            (_, part) => out.push(part),
        }
    }
    out
}

/// The text of a char literal that spells itself (`'/'`), else a hole.
fn char_text(node: Node, src: &[u8]) -> Vec<Part> {
    let inner = text(node, src).trim_matches('\'');
    match inner.chars().count() == 1 && inner != "\\" {
        true => vec![Part::Text(inner.to_string())],
        false => vec![Part::Hole],
    }
}

/// The name of the `Kind::Inline` definition the closure `node`, written as an argument within
/// `scope`, makes.
pub(super) fn inline(scope: &str, node: Node) -> String {
    qualify(scope, &format!("@{}", node.start_byte()))
}

/// A string literal as one text part; `Unknown` for one with an escape sequence, since its
/// bytes are not what the source spells.
fn string(node: Node, src: &[u8]) -> Value {
    let mut out = String::new();
    for part in named(node) {
        match part.kind() {
            "string_content" => out.push_str(text(part, src)),
            _ => return Value::Unknown,
        }
    }
    Value::Str(vec![Part::Text(out)])
}
