//! The values the Go pack reads off an expression: what a call is given, what a function returns
//! and what a `:=` holds. Only what the syntax says outright: a string literal, a `+` of strings,
//! a `fmt.Sprintf` with a literal format, a `net/http` method constant, a name, a call, a
//! function literal. Anything else is `Unknown`, so no kind reads a guess.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use tree_sitter::Node;

use super::{DEFINITION_DEPTH, callee};
use crate::lang::{Import, Invocation, Part, Value, named, text};
use crate::qualify;

/// How many nested expressions a value is read through (`f(g(h(..)))`); a deeper one is
/// `Unknown`.
// ponytail: real arguments nest a handful of calls; the limit keeps a crafted one cheap.
pub(super) const VALUE_DEPTH: usize = 64;

/// The packages of the standard library whose values are folded where they are written
/// (`fmt.Sprintf`, `http.MethodGet`, `os.Getenv`, `strings.TrimSuffix`): the name the file
/// imports each by, unless the file spells that name some other way too.
#[derive(Default)]
pub(super) struct Std {
    fmt: Option<String>,
    http: Option<String>,
    os: Option<String>,
    strings: Option<String>,
    /// The names a statement of a function's own block binds once, by scope and name (`base :=
    /// os.Getenv("X")`), with whether the value is an environment read. Filled in source order,
    /// so a name is known where it is used after it is bound.
    held: RefCell<HashMap<(String, String), bool>>,
}

impl Std {
    pub(super) fn new(root: Node, src: &[u8], imports: &[Import]) -> Std {
        let by = |source: &str| {
            let local = source.rsplit('/').next().unwrap_or(source);
            let mut named = imports.iter().filter(|i| i.local == local);
            let one = named.next().filter(|i| i.source == source);
            one.filter(|_| named.next().is_none())
                .map(|_| local.to_string())
        };
        let (fmt, http) = (by("fmt"), by("net/http"));
        let (os, strings) = (by("os"), by("strings"));
        let all = [&fmt, &http, &os, &strings];
        let names: Vec<&str> = all.iter().filter_map(|n| n.as_deref()).collect();
        let hidden = if names.is_empty() {
            BTreeSet::new()
        } else {
            hidden(root, src, &names)
        };
        let free = |name: Option<String>| name.filter(|n| !hidden.contains(n));
        Std {
            fmt: free(fmt),
            http: free(http),
            os: free(os),
            strings: free(strings),
            held: RefCell::default(),
        }
    }

    /// Notes that `name`, bound in `scope`, holds `value`.
    pub(super) fn note(&self, scope: &str, name: &str, value: &Value) {
        let env = is_env(value);
        (self.held.borrow_mut()).insert((scope.to_string(), name.to_string()), env);
    }

    /// Whether `name`, written in `scope`, is bound by the nearest statement noted for it to an
    /// environment read.
    fn holds_env(&self, scope: &str, name: &str) -> bool {
        let held = self.held.borrow();
        let mut at = scope;
        loop {
            if let Some(env) = held.get(&(at.to_string(), name.to_string())) {
                return *env;
            }
            if at.is_empty() {
                return false;
            }
            at = crate::parent(at);
        }
    }
}

/// Whether `value` is nothing but one environment read.
pub(super) fn is_env(value: &Value) -> bool {
    matches!(value, Value::Str(parts) if matches!(parts[..], [Part::Env(_)]))
}

/// Which of `names` the file spells other than as the package of a selector (`fmt.Sprintf`): a
/// parameter, a variable, a function. Any of them may hide the import.
// ponytail: every such spelling counts as a binding, whatever its scope; it errs toward leaving
// the call a plain call.
fn hidden(root: Node, src: &[u8], names: &[&str]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.kind() == "identifier" && names.contains(&text(node, src)) {
            let package = node.parent().is_some_and(|p| {
                p.kind() == "selector_expression" && p.child_by_field_name("operand") == Some(node)
            });
            if !package {
                out.insert(text(node, src).to_string());
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return out;
            }
        }
    }
}

/// The value of `node`, written in `scope` at function literal nesting `depth` (a literal past
/// `DEFINITION_DEPTH` is no definition, so it is no function value).
pub(super) fn value(node: Node, src: &[u8], std: &Std, scope: &str, depth: usize) -> Value {
    read(node, (src, std), scope, depth, 0)
}

fn read(node: Node, files: (&[u8], &Std), scope: &str, depth: usize, level: usize) -> Value {
    let (src, std) = files;
    if level >= VALUE_DEPTH {
        return Value::Unknown;
    }
    let again = |n: Node| read(n, files, scope, depth, level + 1);
    match node.kind() {
        "raw_string_literal" => {
            let raw = text(node, src);
            string(raw.get(1..raw.len().saturating_sub(1)))
        }
        "interpreted_string_literal" => {
            let quoted = text(node, src);
            string(
                quoted
                    .get(1..quoted.len().saturating_sub(1))
                    .filter(|s| !s.contains('\\')),
            )
        }
        "identifier" => Value::Ref(text(node, src).to_string()),
        "selector_expression" => match callee(node, src) {
            name if name.starts_with('(') => Value::Unknown,
            name => method_constant(&name, std).unwrap_or(Value::Ref(name)),
        },
        "binary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|o| text(o, src) == "+") =>
        {
            // By value: `&again` here runs clippy's needless_borrows_for_generic_args out of memory.
            let side = |field| {
                node.child_by_field_name(field)
                    .map_or(Value::Unknown, again)
            };
            concat(side("left"), side("right"))
        }
        "parenthesized_expression" => named(node).next().map_or(Value::Unknown, again),
        "unary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|o| text(o, src) == "&") =>
        {
            node.child_by_field_name("operand")
                .map_or(Value::Unknown, again)
        }
        "func_literal" if depth < DEFINITION_DEPTH => {
            Value::Function(qualify(scope, &format!("@{}", node.start_byte())))
        }
        "call_expression" => {
            let Some(function) = node.child_by_field_name("function") else {
                return Value::Unknown;
            };
            let sprintf = std.fmt.as_ref().map(|fmt| format!("{fmt}.Sprintf"));
            if sprintf.is_some_and(|name| callee(function, src) == name) {
                return sprintf_value(node, src, again);
            }
            if let Some(env) = env_value(node, function, files, scope, again) {
                return env;
            }
            Value::Call(invocation(node, function, src, again))
        }
        _ => Value::Unknown,
    }
}

/// The environment read the call `node` is, or the trimmed one: `os.Getenv("X")`, and
/// `strings.TrimSuffix(x, "/")`, `strings.TrimRight(x, "/")` or `strings.TrimSpace(x)` of one.
/// The first is the value `Str([Env(X)])`; a trim keeps the read it trims, a name bound to a
/// read included. Any other call of these functions (a name that is no literal, another
/// argument that is no text, a trim of another value) is `Unknown`, and its arguments are not
/// read again as a call's. `None` for any other call. Meada never reads the variable.
fn env_value(
    node: Node,
    function: Node,
    files: (&[u8], &Std),
    scope: &str,
    again: impl Fn(Node) -> Value + Copy,
) -> Option<Value> {
    let (src, std) = files;
    let name = callee(function, src);
    let (package, member) = name.split_once('.')?;
    let args: Vec<Node> = arguments(node).collect();
    if std.os.as_deref() == Some(package) && member == "Getenv" {
        let read = env_name(&args, again).map(|env| Value::Str(vec![env]));
        return Some(read.unwrap_or(Value::Unknown));
    }
    if std.strings.as_deref() != Some(package) {
        return None;
    }
    let width = match member {
        "TrimSuffix" | "TrimRight" => 2,
        "TrimSpace" => 1,
        _ => return None,
    };
    Some(trimmed(&args, width, files, scope, again).unwrap_or(Value::Unknown))
}

/// The read a trim of `width` arguments keeps, if it trims one.
fn trimmed(
    args: &[Node],
    width: usize,
    files: (&[u8], &Std),
    scope: &str,
    again: impl Fn(Node) -> Value + Copy,
) -> Option<Value> {
    if args.len() != width {
        return None;
    }
    // The text trimmed away must be text: a second read there would make two bases.
    if args.get(1).is_some_and(
        |a| !matches!(again(*a), Value::Str(p) if p.iter().all(|p| matches!(p, Part::Text(_)))),
    ) {
        return None;
    }
    match again(args[0]) {
        env @ Value::Str(_) if is_env(&env) => Some(env),
        Value::Ref(held) if files.1.holds_env(scope, &held) => Some(Value::Ref(held)),
        _ => None,
    }
}

/// The variable `os.Getenv("X")` or `os.LookupEnv("X")` names, when its only argument is a
/// non-empty string literal.
fn env_name(args: &[Node], again: impl Fn(Node) -> Value) -> Option<Part> {
    let [name] = args else {
        return None;
    };
    match again(*name) {
        Value::Str(parts) => match <[Part; 1]>::try_from(parts) {
            Ok([Part::Text(name)]) if !name.is_empty() => Some(Part::Env(name)),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the assignment `node` leaves a name that holds an environment read bound to it: the
/// fallback `if v == "" { v = .. }` (whatever the default is, Meada never reads it, unless it is
/// another environment read), or the trim of the name by itself (`v = strings.TrimSuffix(v,
/// "/")`).
pub(super) fn keeps_env(node: Node, src: &[u8], std: &Std) -> bool {
    let sides = (node.child_by_field_name("left"))
        .zip(node.child_by_field_name("right"))
        .zip(node.child_by_field_name("operator"));
    let Some(((left, right), operator)) = sides else {
        return false;
    };
    let (Some(target), Some(right)) = (only(left), only(right)) else {
        return false;
    };
    if text(operator, src) != "=" || target.kind() != "identifier" {
        return false;
    }
    let name = text(target, src);
    trims_itself(right, src, std, name) || empties_to(node, src, name) && !reads_env(right, src)
}

/// The one expression of `list`, which is itself when it is no expression list.
fn only(list: Node) -> Option<Node> {
    match list.kind() {
        "expression_list" => {
            let mut all = named(list);
            all.next().filter(|_| all.next().is_none())
        }
        _ => Some(list),
    }
}

/// Whether `node` is `strings.TrimSuffix(name, "..")`, `strings.TrimRight(name, "..")` or
/// `strings.TrimSpace(name)`.
fn trims_itself(node: Node, src: &[u8], std: &Std, name: &str) -> bool {
    let Some(function) = node.child_by_field_name("function") else {
        return false;
    };
    let Some(strings) = std.strings.as_deref() else {
        return false;
    };
    let args: Vec<Node> = arguments(node).collect();
    let text_of = |a: &Node| {
        matches!(
            a.kind(),
            "interpreted_string_literal" | "raw_string_literal"
        )
    };
    match (callee(function, src).strip_prefix(strings), &args[..]) {
        (Some(".TrimSpace"), [first]) => text(*first, src) == name,
        (Some(".TrimSuffix" | ".TrimRight"), [first, cut]) => {
            text(*first, src) == name && text_of(cut)
        }
        _ => false,
    }
}

/// Whether the assignment `node` is the only statement of `if name == "" { .. }`, with no
/// initializer and no `else`.
fn empties_to(node: Node, src: &[u8], name: &str) -> bool {
    let block = node.parent().filter(|p| p.kind() == "statement_list");
    let alone = block.is_some_and(|b| named(b).count() == 1);
    let block = block
        .and_then(|b| b.parent())
        .filter(|b| b.kind() == "block");
    let branch = block.and_then(|b| b.parent()).filter(|i| {
        i.kind() == "if_statement"
            && i.child_by_field_name("alternative").is_none()
            && i.child_by_field_name("initializer").is_none()
            && i.child_by_field_name("consequence") == block
    });
    let condition = branch.and_then(|i| i.child_by_field_name("condition"));
    let Some(condition) = condition.filter(|c| c.kind() == "binary_expression") else {
        return false;
    };
    let (left, right) = (
        condition.child_by_field_name("left"),
        condition.child_by_field_name("right"),
    );
    let operator = condition.child_by_field_name("operator");
    let empty = |n: Node| matches!(text(n, src), "\"\"" | "``");
    let sides = left.zip(right).is_some_and(|(l, r)| {
        (text(l, src) == name && empty(r)) || (empty(l) && text(r, src) == name)
    });
    alone && sides && operator.is_some_and(|o| text(o, src) == "==")
}

/// Whether the expression `node` names `Getenv` or `LookupEnv` anywhere.
fn reads_env(node: Node, src: &[u8]) -> bool {
    // An explicit stack: a crafted expression can nest deeper than the call stack allows.
    let mut stack = vec![node];
    while let Some(at) = stack.pop() {
        if matches!(at.kind(), "identifier" | "field_identifier")
            && matches!(text(at, src), "Getenv" | "LookupEnv")
        {
            return true;
        }
        stack.extend(named(at));
    }
    false
}

/// The first result of `os.LookupEnv("X")`, the call `node` as the one value of a `:=` or `var`.
pub(super) fn lookup_env(
    node: Node,
    src: &[u8],
    std: &Std,
    scope: &str,
    depth: usize,
) -> Option<Value> {
    let function = node.child_by_field_name("function")?;
    let os = std.os.as_deref()?;
    if node.kind() != "call_expression" || callee(function, src) != format!("{os}.LookupEnv") {
        return None;
    }
    let args: Vec<Node> = arguments(node).collect();
    let again = |n: Node| read(n, (src, std), scope, depth, 1);
    env_name(&args, again).map(|env| Value::Str(vec![env]))
}

/// What `http.MethodGet` and the other `net/http` method constants spell.
fn method_constant(name: &str, std: &Std) -> Option<Value> {
    const METHODS: &[&str] = &[
        "Get", "Head", "Post", "Put", "Patch", "Delete", "Connect", "Options", "Trace",
    ];
    let (package, constant) = name.split_once('.')?;
    let method = constant.strip_prefix("Method")?;
    if std.http.as_deref() != Some(package) || !METHODS.contains(&method) {
        return None;
    }
    Some(Value::Str(vec![Part::Text(method.to_uppercase())]))
}

/// The parts a value gives the string it is part of: its own, a name to fold, or a hole.
fn pieces(value: Value) -> Vec<Part> {
    match value {
        Value::Str(parts) => parts,
        Value::Ref(name) => vec![Part::Ref(name)],
        _ => vec![Part::Hole],
    }
}

/// `parts` with the text next to text joined.
fn joined(parts: impl IntoIterator<Item = Part>) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for part in parts {
        match (out.last_mut(), part) {
            (_, Part::Text(text)) if text.is_empty() => {}
            (Some(Part::Text(before)), Part::Text(text)) => before.push_str(&text),
            (_, part) => out.push(part),
        }
    }
    out
}

/// `left + right`: a string when either side is one or a name (a sum of numbers is neither).
fn concat(left: Value, right: Value) -> Value {
    let string = |v: &Value| matches!(v, Value::Str(_) | Value::Ref(_));
    if !(string(&left) || string(&right)) {
        return Value::Unknown;
    }
    Value::Str(joined(pieces(left).into_iter().chain(pieces(right))))
}

/// An argument of `fmt.Sprintf`: a decimal integer literal, or any other value.
enum Arg {
    Int(String),
    Other(Value),
}

/// `fmt.Sprintf(format, args..)` with a literal format: a bare `%s` or `%v` is the argument at
/// its place when that is a string, `%d` and `%v` an integer literal, and `%%` is `%`. Any other
/// pairing of a verb and an argument, and a verb with flags, width or precision (`%5s`, `%.1s`,
/// `%#v`), is a hole. A format with another verb, a number of arguments other than its verbs,
/// or `args...` is `Unknown`.
fn sprintf_value(node: Node, src: &[u8], read: impl Fn(Node) -> Value) -> Value {
    let args: Vec<Node> = arguments(node).collect();
    if args.iter().any(|a| a.kind() == "variadic_argument") {
        return Value::Unknown;
    }
    let Some(Value::Str(format)) = args.first().map(|a| read(*a)) else {
        return Value::Unknown;
    };
    let [Part::Text(format)] = &format[..] else {
        return Value::Unknown;
    };
    let arg = |a: &Node| {
        let digits = text(*a, src);
        let decimal = digits == "0" || digits.starts_with(|c: char| ('1'..='9').contains(&c));
        match a.kind() == "int_literal" && decimal && digits.chars().all(|c| c.is_ascii_digit()) {
            true => Arg::Int(digits.to_string()),
            false => Arg::Other(read(*a)),
        }
    };
    format_parts(format, &mut args[1..].iter().map(arg))
        .map_or(Value::Unknown, |parts| Value::Str(joined(parts)))
}

/// The parts of `format` with `given` at its verbs, `None` when it has any other verb or more or
/// fewer verbs than `given` has values.
fn format_parts(format: &str, given: &mut impl Iterator<Item = Arg>) -> Option<Vec<Part>> {
    let (mut parts, mut text) = (Vec::new(), String::new());
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            text.push(c);
            continue;
        }
        let mut verb = chars.next();
        if verb == Some('%') {
            text.push('%');
            continue;
        }
        let mut plain = true;
        while verb.is_some_and(|v| matches!(v, '+' | '-' | '#' | ' ' | '.' | '0'..='9')) {
            (plain, verb) = (false, chars.next());
        }
        let verb @ ('s' | 'd' | 'v') = verb? else {
            return None;
        };
        parts.push(Part::Text(std::mem::take(&mut text)));
        parts.extend(match (given.next()?, verb, plain) {
            (Arg::Int(digits), 'd' | 'v', true) => vec![Part::Text(digits)],
            (Arg::Other(value), 's' | 'v', true) => pieces(value),
            _ => vec![Part::Hole],
        });
    }
    parts.push(Part::Text(text));
    given.next().is_none().then_some(parts)
}

/// The text of a string literal's content as a value, `Unknown` when it has none to read.
fn string(content: Option<&str>) -> Value {
    content.map_or(Value::Unknown, |c| {
        Value::Str(vec![Part::Text(c.to_string())])
    })
}

/// The call `node` of `function`, with the value it is made on and its arguments.
fn invocation(node: Node, function: Node, src: &[u8], read: impl Fn(Node) -> Value) -> Invocation {
    let operand = (function.kind() == "selector_expression")
        .then(|| function.child_by_field_name("operand"))
        .flatten();
    Invocation {
        callee: callee(function, src),
        receiver: operand.map(|o| Box::new(read(o))),
        args: arguments(node).map(&read).collect(),
        range: node.byte_range(),
    }
}

/// The arguments of the call `node`.
pub(super) fn arguments(node: Node) -> impl Iterator<Item = Node> {
    let list = node.child_by_field_name("arguments");
    list.into_iter().flat_map(named)
}

/// The value a call is made on, when it is a method or a function of a package
/// (`r.Group("/x")`, `http.HandleFunc(..)`).
pub(super) fn receiver(
    function: Node,
    src: &[u8],
    std: &Std,
    scope: &str,
    depth: usize,
) -> Option<Value> {
    if function.kind() != "selector_expression" {
        return None;
    }
    let operand = function.child_by_field_name("operand")?;
    Some(value(operand, src, std, scope, depth))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguagePack;

    /// How many calls deep the argument of the call `f` is read.
    fn read_depth(nesting: usize) -> usize {
        let src = format!(
            "package x\n\nfunc f() {{ {}1{} }}\n",
            "g(".repeat(nesting),
            ")".repeat(nesting)
        );
        let facts = crate::lang::go::Go.parse("x.go", src.as_bytes()).unwrap();
        let outer = facts
            .calls
            .iter()
            .find(|c| c.range.start == src.find("g(").unwrap())
            .unwrap();
        let mut value = outer.args.first();
        let mut depth = 0;
        while let Some(Value::Call(call)) = value {
            depth += 1;
            value = call.args.first();
        }
        depth
    }

    #[test]
    fn calls_nested_past_the_limit_are_unknown() {
        assert_eq!(read_depth(10), 9);
        // The outermost call is the fact; its argument is read to the limit.
        assert_eq!(read_depth(VALUE_DEPTH + 10), VALUE_DEPTH);
    }

    /// The first argument of the call `g` in `src`, folded.
    fn first_arg(src: &str) -> Value {
        let facts = crate::lang::go::Go.parse("x.go", src.as_bytes()).unwrap();
        let call = facts
            .calls
            .iter()
            .find(|c| c.callee.ends_with('g'))
            .unwrap();
        call.args[0].clone()
    }

    #[test]
    fn trims_of_a_read_nested_past_the_limit_are_no_read() {
        let imports = "import (\"os\"\n\"strings\")\n";
        let nested = |n: usize| {
            let read = format!(
                "{}os.Getenv(\"X\"){}",
                "strings.TrimSpace(".repeat(n),
                ")".repeat(n)
            );
            first_arg(&format!("package x\n{imports}func f(){{ g({read}) }}\n"))
        };
        assert!(is_env(&nested(10)));
        assert!(!is_env(&nested(VALUE_DEPTH + 10)));
    }

    #[test]
    fn fmt_and_http_fold_only_as_the_imports_they_name() {
        let imports = "import (\"fmt\"\n\"net/http\")\n";
        let folded = |body: &str| first_arg(&format!("package x\n{imports}func f(){{ {body} }}\n"));
        assert!(matches!(
            folded("g(fmt.Sprintf(\"/a/%d\", 1))"),
            Value::Str(_)
        ));
        assert!(matches!(folded("g(http.MethodGet)"), Value::Str(_)));
        // A local or a parameter of the same name is not the package.
        assert!(!matches!(
            folded("fmt := mk(); g(fmt.Sprintf(\"/a\"))"),
            Value::Str(_)
        ));
        let shadow = "package x\nimport \"net/http\"\nfunc f(http T){ g(http.MethodGet) }\n";
        assert!(!matches!(first_arg(shadow), Value::Str(_)));
    }

    /// The text of what `fmt.Sprintf(format, ..)` gives for the arguments `given`, a name as
    /// `<name>`, a hole as `{}`; `None` when it is not read. Each character of `given` is an
    /// argument: `a` a name, `u` an unreadable value, `i` the integer literal 7, `t` the
    /// string literal "ab".
    fn formatted(format: &str, given: &str) -> Option<String> {
        let arg = |c: char| match c {
            'a' => Arg::Other(Value::Ref("a".into())),
            'i' => Arg::Int("7".into()),
            't' => Arg::Other(Value::Str(vec![Part::Text("ab".into())])),
            _ => Arg::Other(Value::Unknown),
        };
        let parts = format_parts(format, &mut given.chars().map(arg))?;
        Some(
            joined(parts)
                .iter()
                .map(|p| match p {
                    Part::Text(t) => t.clone(),
                    Part::Ref(r) => format!("<{r}>"),
                    Part::Env(_) | Part::Hole => "{}".into(),
                })
                .collect(),
        )
    }

    #[test]
    fn sprintf_verbs_are_holes_in_place() {
        let cases: [(&str, &str, Option<&str>); 22] = [
            ("/users/%s", "a", Some("/users/<a>")),
            ("/users/%d/items/%v", "iu", Some("/users/7/items/{}")),
            ("%s/users", "t", Some("ab/users")),
            ("/v%d/%v", "ii", Some("/v7/7")),
            ("/rate/100%%", "", Some("/rate/100%")),
            ("/users", "", Some("/users")),
            // A verb with flags, width or precision does not print its argument as it is.
            ("/u/%5d/%-5s/%+v", "iat", Some("/u/{}/{}/{}")),
            ("/pad/%5s", "t", Some("/pad/{}")),
            ("/prec/%.1s", "t", Some("/prec/{}")),
            ("/quote/%#v", "t", Some("/quote/{}")),
            // A verb meets only the arguments it prints as they are.
            ("/n/%d", "t", Some("/n/{}")),
            ("/n/%d", "a", Some("/n/{}")),
            ("/s/%s", "i", Some("/s/{}")),
            ("/s/%s", "u", Some("/s/{}")),
            ("/rate/%.2f", "a", None),
            ("/rate/%", "", None),
            ("/users/%s", "", None),
            ("/users", "a", None),
            ("/users/%[1]d", "a", None),
            ("/users/%*d", "a", None),
            ("/users/%x", "a", None),
            ("/users/%s/%s", "a", None),
        ];
        for (format, given, expected) in cases {
            assert_eq!(
                formatted(format, given).as_deref(),
                expected,
                "{format} {given}"
            );
        }
    }
}
