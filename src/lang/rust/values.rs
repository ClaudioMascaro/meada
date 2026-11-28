use std::collections::HashMap;

use tree_sitter::Node;

use super::builders::{BUILDER_DEPTH, BUILDER_SIZE, BUILDER_WORK, Builder, size};
use super::{
    DEFINITION_DEPTH, Heads, PATH_DEPTH, Path, Scan, VERBS, field, levels, member, normalize,
    segments,
};
use crate::lang::{Invocation, Part, Value, children, named, text};
use crate::qualify;

// ponytail: real code wraps a value a few levels; a crafted file must not recurse without end.
pub(super) const VALUE_DEPTH: usize = 64;

// ponytail: real code lists a few hundred handlers and nine methods at most; a crafted file
// must not multiply them with the routes of a function.
pub(super) const ROUTE_ITEMS: usize = 1024;
pub(super) const ROUTE_METHODS: usize = 16;

const METHOD_CRATES: &[&str] = &["reqwest", "http", "hyper", "axum", "actix_web"];

const KEEPS: &[&str] = &["to_string", "to_owned", "into", "clone", "as_str", "as_ref"];
const KEEPS_NAMES: &[&str] = &["to_string", "to_owned", "as_str"];

const ROUTES: &str = "routes";

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

const FORMATS: &[&str] = &["format", "concat"];

// ponytail: a URL is built from a few arguments; a crafted file with 300k `{u}` captures must
// not copy a builder for each.
pub(super) const FORMAT_ARGS: usize = 64;
pub(super) const FORMAT_PLACEHOLDERS: usize = 256;

const FORMAT_SIZE: usize = BUILDER_SIZE;

pub(super) struct Where {
    pub scope: String,
    pub module: String,
    pub pure: bool,
}

pub(super) type Builders<'t> = HashMap<(String, String), Builder<'t>>;

pub(super) struct Reader<'a, 't> {
    src: &'a [u8],
    scan: &'a Scan,
    heads: &'a mut Heads,
    builders: &'a Builders<'t>,
    folded: HashMap<(String, String, usize), Vec<Part>>,
    prefixes: HashMap<(String, String), Vec<Vec<Part>>>,
    nest: usize,
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

    pub fn value(&mut self, at: &Where, node: Node<'t>) -> Value {
        self.value_at(at, node, 0)
    }

    pub fn query_write(
        &mut self,
        at: &Where,
        node: Node<'t>,
        name: &str,
        operator: &str,
    ) -> crate::lang::QueryWrite {
        let builders = Builders::new();
        let mut syntax = Reader::new(self.src, self.scan, self.heads, &builders);
        crate::lang::QueryWrite::of(&syntax.value(at, node), name, operator)
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
                "array_expression" | "tuple_expression" => {
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
            "if_expression" => {
                let single = |branch: Node<'t>| {
                    let block = if branch.kind() == "else_clause" {
                        named(branch).next()?
                    } else {
                        branch
                    };
                    let mut statements = named(block).filter(|n| !is_comment(*n));
                    let one = statements.next()?;
                    (block.kind() == "block" && statements.next().is_none()).then_some(one)
                };
                if let (Some(yes), Some(no)) = (
                    field(node, "consequence").and_then(single),
                    field(node, "alternative").and_then(single),
                ) {
                    let yes = self.value_at(at, yes, level + 1);
                    let no = self.value_at(at, no, level + 1);
                    return crate::lang::query_alternative(yes, no);
                }
                Value::Unknown
            }
            "binary_expression" => self.sum(at, node, level),
            "macro_invocation" => self.macro_value(at, node),
            _ => Value::Unknown,
        }
    }

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
            kwargs: Vec::new(),
            range: node.byte_range(),
        })
    }

    fn sum(&mut self, at: &Where, node: Node<'t>, level: usize) -> Value {
        let plus = field(node, "operator").is_some_and(|o| text(o, self.src) == "+");
        let (Some(left), Some(right), true) = (field(node, "left"), field(node, "right"), plus)
        else {
            return Value::Unknown;
        };
        let sides = [left, right].map(|n| self.value_at(at, n, level + 1));
        if !sides.iter().any(|v| matches!(v, Value::Str(_))) {
            return Value::Unknown;
        }
        let parts = sides.into_iter().flat_map(|side| match side {
            Value::Str(parts) => parts,
            Value::Ref(name) => vec![Part::Ref(name)],
            other => vec![Part::unread(&other)],
        });
        Value::Str(merge(parts))
    }

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

    fn spelled<'s>(&self, at: &Where, segments: &'s [String]) -> Option<Vec<&'s str>>
    where
        'a: 's,
    {
        let head = segments.first()?;
        if segments.len() == 1 && self.bound(at, head) {
            return None;
        }
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

    fn bound(&self, at: &Where, name: &str) -> bool {
        let mut scope = at.scope.as_str();
        loop {
            if self.scan.locals.contains(&qualify(scope, name))
                || self
                    .scan
                    .params
                    .contains(&(scope.to_string(), name.to_string()))
            {
                return true;
            }
            if scope.len() <= at.module.len() {
                return false;
            }
            scope = crate::parent(scope);
        }
    }

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
                let name = self.macro_name(at, field(node, "macro")?)?;
                (name == "env").then(|| self.env_macro(node))?
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
            (Some(name), None) if name.kind() == "identifier" => Some(Part::EnvKey {
                name: text(name, self.src).into(),
                reader: None,
            }),
            (Some(name), None) => self.literal(name).map(Part::Env),
            _ => None,
        }
    }

    fn env_macro(&self, node: Node) -> Option<Part> {
        let tree = named(node).find(|c| c.kind() == "token_tree")?;
        self.env_name(tree).map(Part::Env)
    }

    fn env_name(&self, tree: Node) -> Option<String> {
        match arguments(tree)?.first()?.as_slice() {
            [name] => self.literal(*name),
            _ => None,
        }
    }

    fn literal(&self, node: Node) -> Option<String> {
        match string(node, self.src) {
            Value::Str(parts) => match <[Part; 1]>::try_from(parts) {
                Ok([Part::Text(text)]) => Some(text),
                _ => None,
            },
            _ => None,
        }
    }

    fn macro_value(&mut self, at: &Where, node: Node<'t>) -> Value {
        let Some(name) = field(node, "macro").and_then(|m| self.macro_name(at, m)) else {
            return Value::Unknown;
        };
        if name == ROUTES {
            return self.routes(at, node);
        }
        if name == "env" {
            return self
                .env_macro(node)
                .map_or(Value::Unknown, |e| Value::Str(vec![e]));
        }
        if !FORMATS.contains(&name.as_str()) {
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

    fn routes(&mut self, at: &Where, node: Node<'t>) -> Value {
        let Some(args) = named(node)
            .find(|c| c.kind() == "token_tree")
            .and_then(arguments)
        else {
            return Value::Unknown;
        };
        if args.len() > ROUTE_ITEMS {
            return Value::Unknown;
        }
        let items: Vec<Value> = (args.iter())
            .map(|tokens| self.route_item(at, tokens))
            .collect();
        Value::List(items)
    }

    fn route_item(&mut self, at: &Where, tokens: &[Node<'t>]) -> Value {
        let mut segments = Vec::new();
        for (n, token) in tokens.iter().enumerate().take(PATH_DEPTH * 2) {
            match (n % 2, token.kind()) {
                (0, "identifier" | "crate" | "self" | "super") => {
                    segments.push(text(*token, self.src).to_string());
                }
                (1, "::") => {}
                _ => return Value::Unknown,
            }
        }
        if tokens.len().is_multiple_of(2) || tokens.len() > PATH_DEPTH * 2 {
            return Value::Unknown;
        }
        match segments.len() {
            0 => Value::Unknown,
            1 => Value::Ref(segments.remove(0)),
            _ => {
                let path = self.path_of(at, segments);
                match normalize(&path, self.scan, self.heads, false) {
                    Some(name) => Value::Ref(name),
                    None => Value::Unknown,
                }
            }
        }
    }

    fn macro_name(&self, at: &Where, path: Node) -> Option<String> {
        let name = match path.kind() {
            "identifier" => text(path, self.src).to_string(),
            "scoped_identifier" => {
                let segments = segments(path, self.src)?;
                match segments.as_slice() {
                    [krate, name] if matches!(krate.as_str(), "std" | "core" | "alloc") => {
                        name.clone()
                    }
                    [krate, name] if krate == "rocket" && name == ROUTES => name.clone(),
                    _ => return None,
                }
            }
            _ => return None,
        };
        let rocket = name == ROUTES
            && self
                .target(at, &name)
                .is_some_and(|t| t.as_slice() == ["rocket", ROUTES]);
        (path.kind() == "scoped_identifier" || rocket || self.std_macro(at, &name)).then_some(name)
    }

    fn std_macro(&self, at: &Where, name: &str) -> bool {
        if self.scan.macros.contains(name) {
            return false;
        }
        if !self
            .scan
            .defines(&self.path_of(at, vec![name.to_string()]), name)
        {
            return true;
        }
        matches!(
            self.target(at, name).map(Vec::as_slice),
            Some([krate, last]) if matches!(krate.as_str(), "std" | "core" | "alloc") && last == name
        )
    }

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

    fn argument(&mut self, at: &Where, nodes: &[Node<'t>]) -> Vec<Part> {
        if let [name, bang, tree] = nodes
            && [name.kind(), bang.kind(), tree.kind()] == ["identifier", "!", "token_tree"]
            && text(*name, self.src) == "env"
            && self.std_macro(at, "env")
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

    fn capture(&mut self, at: &Where, name: &str, pos: usize) -> Vec<Part> {
        match self.built(at, name, pos) {
            Some(parts) => parts,
            None => vec![Part::Ref(name.to_string())],
        }
    }

    fn built(&mut self, at: &Where, name: &str, pos: usize) -> Option<Vec<Part>> {
        let builders = self.builders;
        if builders.is_empty() {
            return None;
        }
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

    pub fn returns(&mut self, at: &Where, body: Node<'t>) -> Vec<Value> {
        let mut out = Vec::new();
        let tail = named(body).filter(|n| !is_comment(*n)).last();
        out.extend(tail.map(|t| self.value(at, t)));
        let mut stack = vec![body];
        while let Some(node) = stack.pop() {
            match node.kind() {
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

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

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

fn char_text(node: Node, src: &[u8]) -> Vec<Part> {
    let inner = text(node, src).trim_matches('\'');
    match inner.chars().count() == 1 && inner != "\\" {
        true => vec![Part::Text(inner.to_string())],
        false => vec![Part::Hole],
    }
}

pub(super) fn inline(scope: &str, node: Node) -> String {
    qualify(scope, &format!("@{}", node.start_byte()))
}

pub(super) fn attribute(tree: Node, src: &[u8]) -> (Vec<Value>, Vec<(String, Value)>) {
    let (mut args, mut kwargs) = (Vec::new(), Vec::new());
    let literal = |n: &Node| match n.kind() {
        "string_literal" | "raw_string_literal" => string(*n, src),
        _ => Value::Unknown,
    };
    let mut methods = 0;
    for tokens in arguments(tree).unwrap_or_default() {
        match tokens.as_slice() {
            [name, eq, value] if name.kind() == "identifier" && eq.kind() == "=" => {
                let name = text(*name, src).to_string();
                let value = match name == "method" && methods >= ROUTE_METHODS {
                    true => Value::Unknown,
                    false => literal(value),
                };
                methods += usize::from(name == "method");
                kwargs.push((name, value));
            }
            [value] if kwargs.is_empty() => args.push(literal(value)),
            _ => kwargs.push((String::new(), Value::Unknown)),
        }
    }
    (args, kwargs)
}

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
