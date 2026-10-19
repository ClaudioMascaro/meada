//! The values the Go pack reads off an expression: what a call is given, what a function returns
//! and what a `:=` holds. Only what the syntax says outright: a string literal, a `+` of strings,
//! a `fmt.Sprintf` with a literal format, a `net/http` method constant, a name, a call, a
//! function literal. Anything else is `Unknown`, so no kind reads a guess.

use std::collections::BTreeSet;

use tree_sitter::Node;

use super::{DEFINITION_DEPTH, callee};
use crate::lang::{Import, Invocation, Part, Value, named, text};
use crate::qualify;

/// How many nested expressions a value is read through (`f(g(h(..)))`); a deeper one is
/// `Unknown`.
// ponytail: real arguments nest a handful of calls; the limit keeps a crafted one cheap.
pub(super) const VALUE_DEPTH: usize = 64;

/// The packages of the standard library whose values are folded where they are written
/// (`fmt.Sprintf`, `http.MethodGet`): the name the file imports each by, unless the file spells
/// that name some other way too.
#[derive(Default)]
pub(super) struct Std {
    fmt: Option<String>,
    http: Option<String>,
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
        let names: Vec<&str> = fmt.iter().chain(&http).map(String::as_str).collect();
        let hidden = if names.is_empty() {
            BTreeSet::new()
        } else {
            hidden(root, src, &names)
        };
        let free = |name: Option<String>| name.filter(|n| !hidden.contains(n));
        Std {
            fmt: free(fmt),
            http: free(http),
        }
    }
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
                return sprintf_value(node, again);
            }
            Value::Call(invocation(node, function, src, again))
        }
        _ => Value::Unknown,
    }
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

/// `fmt.Sprintf(format, args..)` with a literal format: each `%s`, `%d` and `%v` (with flags,
/// width and precision) is the argument at its place, and `%%` is `%`. A format with another
/// verb, a number of arguments other than its verbs, or `args...` is `Unknown`.
fn sprintf_value(node: Node, read: impl Fn(Node) -> Value) -> Value {
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
    format_parts(format, &mut args[1..].iter().map(|a| read(*a)))
        .map_or(Value::Unknown, |parts| Value::Str(joined(parts)))
}

/// The parts of `format` with `given` at its verbs, `None` when it has any other verb or more or
/// fewer verbs than `given` has values.
fn format_parts(format: &str, given: &mut impl Iterator<Item = Value>) -> Option<Vec<Part>> {
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
        while verb.is_some_and(|v| matches!(v, '+' | '-' | '#' | ' ' | '.' | '0'..='9')) {
            verb = chars.next();
        }
        let ('s' | 'd' | 'v') = verb? else {
            return None;
        };
        parts.push(Part::Text(std::mem::take(&mut text)));
        parts.extend(pieces(given.next()?));
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

    /// The text of what `fmt.Sprintf(format, a, b)` gives, a name as `<name>`, a hole as `{}`;
    /// `None` when it is not read.
    fn formatted(format: &str, given: &[Value]) -> Option<String> {
        let parts = format_parts(format, &mut given.iter().cloned())?;
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
        let (name, unknown) = (Value::Ref("a".into()), Value::Unknown);
        let given = |n: usize| [name.clone(), unknown.clone(), unknown.clone()][..n].to_vec();
        let cases: [(&str, usize, Option<&str>); 13] = [
            ("/users/%s", 1, Some("/users/<a>")),
            ("/users/%d/items/%v", 2, Some("/users/<a>/items/{}")),
            ("%s/users", 1, Some("<a>/users")),
            ("/u/%5d/%-5s/%+v", 3, Some("/u/<a>/{}/{}")),
            ("/rate/100%%", 0, Some("/rate/100%")),
            ("/users", 0, Some("/users")),
            ("/rate/%.2f", 1, None),
            ("/rate/%", 0, None),
            ("/users/%s", 0, None),
            ("/users", 1, None),
            ("/users/%[1]d", 1, None),
            ("/users/%*d", 1, None),
            ("/users/%x", 1, None),
        ];
        for (format, n, expected) in cases {
            assert_eq!(
                formatted(format, &given(n)).as_deref(),
                expected,
                "{format}"
            );
        }
    }
}
