//! The shape facts of the TypeScript and JavaScript pack (spec #23): what the function making
//! each call does with its result (`Call::uses`), read from the syntax tree while it is at hand.
//! A value is followed through member and element accesses, method calls, `await`, and a
//! `const` or destructuring pattern that holds it, the variable of a `for...of` and the element
//! parameter of an array method's callback, until it is returned or the code does something
//! else with it (passes it to a function, for one).

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::{BODIES, bound_nodes, named, text, types, unquote};
use crate::lang::{Step, Ty};

/// What following values may cost in one file: every node visited, by the climb from a call or
/// by the scan of a function for a name's uses, and every step copied when a `const` is
/// followed. Past it, the file's remaining calls have no uses: they read nothing, so they give
/// no drift.
// ponytail: one budget per file; real files spend a few thousand, a crafted chain of `const`s
// would otherwise cost the square of its length.
const USES_BUDGET: usize = 1 << 20;

/// How deep a destructuring pattern is followed: a name nested deeper than this many fields or
/// elements reads nothing, and nor do the names below it.
// ponytail: real patterns nest a few levels; a crafted one would otherwise write paths as long
// as the file.
const PATTERN_DEPTH: usize = 32;

/// The array methods whose callback takes an element first.
const ELEMENT_METHODS: &[&str] = &[
    "map",
    "forEach",
    "filter",
    "find",
    "findLast",
    "findIndex",
    "findLastIndex",
    "some",
    "every",
    "flatMap",
];

/// The uses of the calls of one file, sharing one budget.
pub(super) struct Uses<'t> {
    src: &'t [u8],
    /// The names bound and used in each function (or the whole file), by node id, built when
    /// a value is first followed into a `const` there.
    scopes: HashMap<usize, Scope<'t>>,
    spent: usize,
}

#[derive(Default)]
struct Scope<'t> {
    /// How many bindings make each name in the function, nested functions included.
    binds: HashMap<&'t str, usize>,
    /// Each identifier that uses a name, in source order.
    uses: HashMap<&'t str, Vec<Node<'t>>>,
}

impl<'t> Uses<'t> {
    pub(super) fn new(src: &'t [u8]) -> Self {
        Self {
            src,
            scopes: HashMap::new(),
            spent: 0,
        }
    }

    fn charge(&mut self, cost: usize) -> bool {
        self.spent = self.spent.saturating_add(cost);
        self.spent <= USES_BUDGET
    }

    /// What the function enclosing `call` does with its result, one chain of steps per use.
    pub(super) fn of(&mut self, call: Node<'t>) -> Vec<Vec<Step>> {
        let function = enclosing(call);
        let mut out = Vec::new();
        let mut followed = HashSet::new();
        let mut pending = vec![(call, Vec::new())];
        // The type arguments of the call itself (`get<T>(path)`): the function called says
        // whether they type what it returns.
        if !self.arguments(call, &mut out) {
            return Vec::new();
        }
        while let Some((node, mut steps)) = pending.pop() {
            let mut at = node;
            while let Some(parent) = at.parent() {
                if !self.charge(1) {
                    return Vec::new();
                }
                let is =
                    |field| (parent.child_by_field_name(field)).is_some_and(|c| c.id() == at.id());
                match parent.kind() {
                    "await_expression" | "parenthesized_expression" | "non_null_expression" => {}
                    "as_expression" | "satisfies_expression"
                        if named(parent).next().is_some_and(|c| c.id() == at.id()) =>
                    {
                        // What the cast declares counts; the reads that follow it go on.
                        let cast = Some(parent).filter(|p| p.kind() == "as_expression");
                        if let Some(t) = cast.and_then(|p| named(p).nth(1))
                            && !self.typed(&steps, t, &mut out)
                        {
                            return Vec::new();
                        }
                    }
                    "member_expression" if is("object") => {
                        let Some(property) = parent.child_by_field_name("property") else {
                            break;
                        };
                        steps.push(Step::Field(text(property, self.src).to_string()));
                    }
                    "subscript_expression" if is("object") => {
                        match parent.child_by_field_name("index") {
                            Some(i) if i.kind() == "number" => steps.push(Step::Element),
                            // A key holding `.` or `[`, or none, has no spelling in a field path.
                            Some(i) if i.kind() == "string" => match unquote(i, self.src) {
                                key if key.is_empty() || key.contains(['.', '[']) => break,
                                key => steps.push(Step::Field(key)),
                            },
                            _ => break,
                        }
                    }
                    "call_expression" if is("function") => match steps.pop() {
                        Some(Step::Field(method)) => {
                            // `json<T>()`: the type argument declares what the method returns.
                            let called = [steps.clone(), vec![Step::Call(method.clone())]].concat();
                            if !self.type_argument(parent, &called, &mut out) {
                                return Vec::new();
                            }
                            if ELEMENT_METHODS.contains(&method.as_str())
                                && let Some((callback, param)) = element_param(parent)
                            {
                                steps.push(Step::Element);
                                out.push(steps.clone());
                                let ok = self.bind(
                                    param,
                                    steps.clone(),
                                    callback,
                                    &mut out,
                                    &mut pending,
                                );
                                if !ok {
                                    return Vec::new();
                                }
                                steps.pop();
                            }
                            steps.push(Step::Call(method));
                        }
                        // The value itself is called: nothing is read from it.
                        _ => {
                            steps.clear();
                            break;
                        }
                    },
                    // What was read so far counts, and so do the uses of the names it is bound to.
                    "variable_declarator" if is("value") => {
                        // An annotation declares the type of the variable.
                        if let Some(t) = parent.child_by_field_name("type")
                            && !self.typed(&steps, t, &mut out)
                        {
                            return Vec::new();
                        }
                        if followed.insert(parent.id())
                            && is_const(parent, self.src)
                            && let Some(name) = parent.child_by_field_name("name")
                            && !self.bind(name, steps.clone(), function, &mut out, &mut pending)
                        {
                            return Vec::new();
                        }
                        break;
                    }
                    // Each element of the value, bound to the loop variable or pattern.
                    "for_in_statement"
                        if is("right")
                            && (parent.child_by_field_name("operator"))
                                .is_some_and(|o| text(o, self.src) == "of") =>
                    {
                        steps.push(Step::Element);
                        out.push(std::mem::take(&mut steps));
                        let element = out[out.len() - 1].clone();
                        if let Some(left) = parent.child_by_field_name("left")
                            && !self.bind(left, element, function, &mut out, &mut pending)
                        {
                            return Vec::new();
                        }
                        break;
                    }
                    "return_statement" => {
                        if enclosing(parent).id() == function.id() {
                            steps.push(Step::Return);
                        }
                        break;
                    }
                    "arrow_function" if is("body") => {
                        if parent.id() == function.id() {
                            steps.push(Step::Return);
                        }
                        break;
                    }
                    _ => break,
                }
                at = parent;
            }
            if !steps.is_empty() {
                out.push(steps);
            }
        }
        out
    }

    /// Adds the chain `steps` then the type `node` writes, unless it lists no fields. `false`
    /// past the budget.
    fn typed(&mut self, steps: &[Step], node: Node<'t>, out: &mut Vec<Vec<Step>>) -> bool {
        let mut cost = 0;
        let ty = types::ty(node, self.src, &mut cost);
        if ty != Ty::Unknown {
            out.push([steps, &[Step::Typed(ty)]].concat());
        }
        self.charge(cost)
    }

    /// Adds the chain of the type arguments `call` is given, unless none lists fields. `false`
    /// past the budget.
    fn arguments(&mut self, call: Node<'t>, out: &mut Vec<Vec<Step>>) -> bool {
        let Some(arguments) = call.child_by_field_name("type_arguments") else {
            return true;
        };
        let mut cost = 0;
        let tys: Vec<Ty> = (named(arguments))
            .map(|t| types::ty(t, self.src, &mut cost))
            .collect();
        if tys.iter().any(|t| *t != Ty::Unknown) {
            out.push(vec![Step::Argument(tys)]);
        }
        self.charge(cost)
    }

    /// The same for the one type argument a method call is given (`json<T>()`): with several,
    /// which one types the result is not known.
    fn type_argument(&mut self, call: Node<'t>, steps: &[Step], out: &mut Vec<Vec<Step>>) -> bool {
        let arguments = call.child_by_field_name("type_arguments");
        match arguments.map(|a| named(a).collect::<Vec<_>>()) {
            Some(only) if only.len() == 1 => self.typed(steps, only[0], out),
            _ => true,
        }
    }

    /// Binds the value read through `steps` to `pattern` in `scope`: each field or element the
    /// pattern names is a read, and the uses of each name it binds are followed. `false` past
    /// the budget.
    fn bind(
        &mut self,
        pattern: Node<'t>,
        steps: Vec<Step>,
        scope: Node<'t>,
        out: &mut Vec<Vec<Step>>,
        pending: &mut Vec<(Node<'t>, Vec<Step>)>,
    ) -> bool {
        let with = |steps: &[Step], step| [steps, &[step]].concat();
        let mut stack = vec![(pattern, steps, 0)];
        while let Some((node, steps, depth)) = stack.pop() {
            // Past the depth limit nothing below is read.
            if depth > PATTERN_DEPTH {
                continue;
            }
            if !self.charge(steps.len() + 1) {
                return false;
            }
            match node.kind() {
                "identifier" | "shorthand_property_identifier_pattern" => {
                    if depth > 0 {
                        out.push(steps.clone());
                    }
                    let found = self.uses_of(node, scope);
                    if !self.charge(steps.len().saturating_mul(found.len())) {
                        return false;
                    }
                    pending.extend(found.into_iter().rev().map(|n| (n, steps.clone())));
                }
                "assignment_pattern" | "object_assignment_pattern" => {
                    stack.extend(node.child_by_field_name("left").map(|l| (l, steps, depth)));
                }
                "object_pattern" => {
                    // Every child copies the steps: charged before they are copied.
                    if !self.charge((steps.len() + 1).saturating_mul(node.named_child_count())) {
                        return false;
                    }
                    for child in named(node) {
                        let (key, value) = match child.kind() {
                            "shorthand_property_identifier_pattern" => {
                                (Some(text(child, self.src).to_string()), Some(child))
                            }
                            "object_assignment_pattern" => (
                                (child.child_by_field_name("left"))
                                    .filter(|l| l.kind() == "shorthand_property_identifier_pattern")
                                    .map(|l| text(l, self.src).to_string()),
                                Some(child),
                            ),
                            "pair_pattern" => (
                                (child.child_by_field_name("key")).and_then(|k| self.key(k)),
                                child.child_by_field_name("value"),
                            ),
                            _ => (None, None),
                        };
                        if let (Some(key), Some(value)) = (key, value) {
                            stack.push((value, with(&steps, Step::Field(key)), depth + 1));
                        }
                    }
                }
                "array_pattern" => {
                    if !self.charge((steps.len() + 1).saturating_mul(node.named_child_count())) {
                        return false;
                    }
                    for child in named(node).filter(|c| c.kind() != "rest_pattern") {
                        stack.push((child, with(&steps, Step::Element), depth + 1));
                    }
                }
                _ => {}
            }
        }
        true
    }

    /// The field a property key of a pattern names, when a field path can write it.
    fn key(&self, key: Node<'t>) -> Option<String> {
        match key.kind() {
            "property_identifier" => Some(text(key, self.src).to_string()),
            // A key holding `.` or `[`, or none, has no spelling in a field path.
            "string" => {
                Some(unquote(key, self.src)).filter(|k| !k.is_empty() && !k.contains(['.', '[']))
            }
            _ => None,
        }
    }

    /// The uses of `name`, an identifier a pattern binds, in `scope`: none when anything else in
    /// the function binds the same name (a parameter, a `catch`, a destructuring, an inner
    /// `let`, an assignment), since a use may then read another value.
    fn uses_of(&mut self, name: Node<'t>, scope: Node<'t>) -> Vec<Node<'t>> {
        let name = text(name, self.src);
        if !self.scopes.contains_key(&scope.id()) {
            let Some(found) = self.scan(scope) else {
                return Vec::new();
            };
            self.scopes.insert(scope.id(), found);
        }
        let found = &self.scopes[&scope.id()];
        if found.binds.get(name) != Some(&1) {
            return Vec::new();
        }
        found.uses.get(name).cloned().unwrap_or_default()
    }

    /// The names `function` binds and uses; `None` past the budget.
    fn scan(&mut self, function: Node<'t>) -> Option<Scope<'t>> {
        let mut scope = Scope::default();
        let mut binding = HashSet::new();
        let mut identifiers = Vec::new();
        // Explicit stack: generated code can nest deeper than the call stack.
        let mut stack = vec![function];
        while let Some(node) = stack.pop() {
            if !self.charge(1) {
                return None;
            }
            let pattern = match node.kind() {
                kind if BODIES.contains(&kind) => node
                    .child_by_field_name("parameters")
                    .or_else(|| node.child_by_field_name("parameter")),
                "catch_clause" => node.child_by_field_name("parameter"),
                "for_in_statement" => node.child_by_field_name("left"),
                "variable_declarator" => node.child_by_field_name("name"),
                // A name assigned again may hold another value by the time it is used.
                "assignment_expression" | "augmented_assignment_expression" => {
                    node.child_by_field_name("left")
                }
                "update_expression" => node.child_by_field_name("argument"),
                _ => None,
            };
            let mut names: Vec<Node> = pattern.map(bound_nodes).unwrap_or_default();
            if matches!(
                node.kind(),
                "function_declaration" | "generator_function_declaration"
            ) {
                names.extend(node.child_by_field_name("name"));
            }
            for name in names {
                binding.insert(name.id());
                *scope.binds.entry(text(name, self.src)).or_default() += 1;
            }
            if node.kind() == "identifier" {
                identifiers.push(node);
            }
            stack.extend(named(node).collect::<Vec<_>>().into_iter().rev());
        }
        for node in identifiers
            .into_iter()
            .filter(|n| !binding.contains(&n.id()))
        {
            let uses = scope.uses.entry(text(node, self.src)).or_default();
            uses.push(node);
        }
        Some(scope)
    }
}

/// Whether `declarator` is part of a `const` declaration.
fn is_const(declarator: Node, src: &[u8]) -> bool {
    (declarator.parent())
        .and_then(|d| d.child_by_field_name("kind"))
        .is_some_and(|k| text(k, src) == "const")
}

/// The function written in place as the first argument of `call`, and the pattern it takes
/// its first parameter as.
fn element_param(call: Node) -> Option<(Node, Node)> {
    let callback = (call.child_by_field_name("arguments")).and_then(|a| named(a).next())?;
    if !BODIES.contains(&callback.kind()) {
        return None;
    }
    let param = (callback.child_by_field_name("parameter")).or_else(|| {
        let first = callback.child_by_field_name("parameters")?.named_child(0)?;
        Some(first.child_by_field_name("pattern").unwrap_or(first))
    })?;
    Some((callback, param))
}

/// The function whose body holds `node`, or the whole file at the top level.
fn enclosing(node: Node) -> Node {
    let mut at = node;
    while let Some(parent) = at.parent() {
        if BODIES.contains(&parent.kind()) {
            return parent;
        }
        at = parent;
    }
    at
}

#[cfg(test)]
mod tests {
    use crate::lang::{LanguagePack, Step, Ty, ts::TsJs};

    fn uses(src: &str) -> Vec<Vec<Step>> {
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let fetch = facts.calls.iter().find(|c| c.callee == "fetch").unwrap();
        fetch.uses.clone()
    }

    #[test]
    fn a_key_the_path_notation_cannot_write_ends_the_read() {
        let read = |key: &str| {
            uses(&format!(
                "async function f() {{\n  const r = await fetch(u);\n  return (await r.json())[{key}].c;\n}}\n"
            ))
        };
        let (json, field) = (Step::Call("json".into()), |f: &str| Step::Field(f.into()));
        assert_eq!(
            read("\"a\""),
            [vec![json.clone(), field("a"), field("c"), Step::Return]]
        );
        for key in ["\"a.b\"", "\"a[0]\"", "'items[]'", "\"\""] {
            assert_eq!(read(key), [vec![json.clone()]], "{key}");
        }
    }

    /// The uses of the fetch in `body`, written as the dotted paths they read after `.json()`.
    fn paths(body: &str) -> Vec<String> {
        let src = format!("async function f() {{\n  const r = await fetch(u);\n{body}\n}}\n");
        let mut out: Vec<String> = (uses(&src).iter())
            .filter(|c| c.first() == Some(&Step::Call("json".into())))
            .map(|chain| {
                (chain[1..].iter())
                    .map(|s| match s {
                        Step::Field(f) => format!(".{f}"),
                        Step::Element => "[]".into(),
                        Step::Call(m) => format!("({m})"),
                        Step::Return => "^".into(),
                        Step::Typed(_) | Step::Argument(_) => "<T>".into(),
                    })
                    .collect()
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn destructuring_reads_each_field_it_names() {
        assert_eq!(
            paths(
                "const { id, owner: { name: who = 'x' }, \"a-b\": c, ...rest } = await r.json();\n  return [who, c.d, rest.z];"
            ),
            ["", ".a-b", ".a-b.d", ".id", ".owner.name"]
        );
        // The names an array pattern binds read elements.
        assert_eq!(
            paths("const [first, { id }] = (await r.json()).items;\n  return first.id;"),
            [".items", ".items[]", ".items[].id", ".items[].id^"]
        );
    }

    #[test]
    fn a_key_the_path_notation_cannot_write_reads_nothing_in_a_pattern() {
        // An empty key, a key with `.`, and a default that is itself a pattern.
        assert_eq!(
            paths(
                "const { \"\": x, \"a.b\": y, c: { \"d[0]\": z }, { \"x.y\": q } = 1 } = await r.json();\n  return [x.a, y.b, z.c];"
            ),
            [""]
        );
    }

    #[test]
    fn a_wide_pattern_over_a_long_chain_is_charged_before_it_is_copied() {
        // Every name copies the steps so far: the product of the two is the cost.
        let names: Vec<String> = (0..30_000).map(|i| format!("a{i}")).collect();
        let chain = ".b".repeat(300);
        assert_eq!(
            paths(&format!(
                "const {{ {} }} = (await r.json(){chain});",
                names.join(", ")
            )),
            Vec::<String>::new()
        );
    }

    #[test]
    fn iteration_reads_the_elements() {
        assert_eq!(
            paths("for (const row of await r.json()) { use(row.id, row.owner.name); }"),
            ["[]", "[].id", "[].owner.name"]
        );
        assert_eq!(
            paths("const list = await r.json();\n  for (const { id } of list.items) {}"),
            ["", ".items[]", ".items[].id"]
        );
        // `for...in` walks keys, not elements.
        assert_eq!(
            paths("for (const k in await r.json()) { use(k.id); }"),
            [""]
        );
    }

    #[test]
    fn array_method_callbacks_read_the_elements() {
        assert_eq!(
            paths(
                "const list = await r.json();\n  list.items.map((x) => x.id);\n  list.rows.forEach(function (row) { use(row.a); });\n  list.rows.filter(({ b }) => b);\n  list.rows.find((x) => x.c);"
            ),
            [
                "",
                ".items(map)",
                ".items[]",
                ".items[].id",
                ".rows(filter)",
                ".rows(find)",
                ".rows(forEach)",
                ".rows[]",
                ".rows[].a",
                ".rows[].b",
                ".rows[].c"
            ]
        );
    }

    #[test]
    fn a_rebound_name_is_not_the_response() {
        // A loop variable assigned again, a callback parameter shadowed by an inner one, a
        // destructured name that a `catch` binds too: the uses of the name read nothing.
        assert_eq!(
            paths(
                "const list = await r.json();\n  for (let row of list) { row = other(); use(row.gone); }\n  list.map((x) => { const f = (x) => x.nope; return x.id; });\n  const { a } = list;\n  try {} catch (a) { use(a.error); }"
            ),
            ["", "(map)", ".a", "[]"]
        );
    }

    #[test]
    fn values_passed_to_another_function_are_not_followed() {
        assert_eq!(
            paths(
                "const list = await r.json();\n  show(list);\n  show(list.items);\n  const { a, b } = list;\n  show(a);\n  list.items.map(show);\n  list.rows.map((x) => { show(x); return show(x.y); });"
            ),
            [
                "",
                ".a",
                ".b",
                ".items",
                ".items(map)",
                ".rows(map)",
                ".rows[]",
                ".rows[].y"
            ]
        );
    }

    #[test]
    fn a_pattern_past_the_depth_limit_reads_nothing_below_it() {
        let nested = |depth: usize| {
            let pattern = (0..depth).fold("leaf".to_string(), |p, _| format!("{{ k: {p} }}"));
            paths(&format!("const {pattern} = await r.json();"))
        };
        let reads = |depth: usize| ".k".repeat(depth);
        assert!(nested(super::PATTERN_DEPTH).contains(&reads(super::PATTERN_DEPTH)));
        // Past the limit the names read nothing: only the value itself is read.
        assert_eq!(nested(super::PATTERN_DEPTH + 8), [""]);
    }

    #[test]
    fn a_chain_of_consts_past_the_budget_reads_nothing() {
        let field = |name: &str| Step::Field(name.into());
        let chain = |n: usize| {
            let mut src = String::from("async function f() {\n  const v0 = await fetch(u);\n");
            for i in 1..=n {
                src += &format!("  const v{i} = v{}.a;\n", i - 1);
            }
            src + &format!("  return v{n}.b;\n}}\n")
        };
        let mut want = vec![field("a"); 100];
        want.extend([field("b"), Step::Return]);
        assert!(uses(&chain(100)).contains(&want));
        // Each `const` copies the steps so far: the cost grows with the square of the chain.
        assert_eq!(uses(&chain(2_000)), Vec::<Vec<Step>>::new());
    }

    #[test]
    fn casts_to_large_types_past_the_budget_read_nothing() {
        let members: String = (0..200).map(|i| format!("f{i}: string; ")).collect();
        let casts = |n: usize| {
            let mut src = String::from("async function f() {\n  const r = await fetch(u);\n");
            for i in 0..n {
                src += &format!("  const d{i} = (await r.json()) as {{ {members} }};\n");
            }
            uses(&(src + "}\n"))
        };
        assert!(!casts(3).is_empty());
        // Each cast reads its type again: the cost grows with the casts times the type.
        assert_eq!(casts(4_000), Vec::<Vec<Step>>::new());
    }

    #[test]
    fn a_declared_type_ends_a_chain_of_its_own() {
        let typed = |name: &str| Step::Typed(Ty::Named(name.into()));
        let (json, field) = (Step::Call("json".into()), |f: &str| Step::Field(f.into()));
        let chains = |body: &str| {
            uses(&format!(
                "async function f() {{\n  const r = await fetch(u);\n{body}\n}}\n"
            ))
        };
        let array = Step::Typed(Ty::Array(Box::new(Ty::Named("Row".into()))));
        let cases = [
            (
                "const d = (await r.json()) as Row[];",
                vec![json.clone(), array.clone()],
            ),
            (
                "const d = (await r.json()).items as Row[];",
                vec![json.clone(), field("items"), array],
            ),
            (
                "const d: Row = await r.json();",
                vec![json.clone(), typed("Row")],
            ),
            (
                "const d = await r.json<Row>();",
                vec![json.clone(), typed("Row")],
            ),
            (
                "let d: Row = (await r.json()) as unknown as Row;",
                vec![json.clone(), typed("Row")],
            ),
        ];
        for (body, chain) in cases {
            assert!(chains(body).contains(&chain), "{body}: {:?}", chains(body));
        }
        // The reads after a cast go on; a form with no fields declares nothing.
        let after = chains("return ((await r.json()) as Row).id;");
        assert!(
            after.contains(&vec![json.clone(), typed("Row")]),
            "{after:?}"
        );
        assert!(
            after.contains(&vec![json.clone(), field("id"), Step::Return]),
            "{after:?}"
        );
        for body in [
            "const d = (await r.json()) as unknown;",
            "const d = (await r.json()) as const;",
            "const d: string | null = await r.json();",
            "const d = await r.json<A, B>();",
        ] {
            assert!(
                !chains(body)
                    .iter()
                    .flatten()
                    .any(|s| matches!(s, Step::Typed(_))),
                "{body}"
            );
        }
    }

    #[test]
    fn a_type_argument_of_the_call_declares_its_result() {
        let facts = TsJs.parse("a.ts", b"async function f() { return get<Row>(u); }\nasync function g() { return get<A, B>(u); }").unwrap();
        let get: Vec<_> = facts.calls.iter().filter(|c| c.callee == "get").collect();
        // The type arguments are offered to the wrapper, which says whether they type the result.
        let named = |n: &str| Ty::Named(n.into());
        let row = Step::Argument(vec![named("Row")]);
        assert_eq!(get[0].uses, [vec![row], vec![Step::Return]]);
        let both = Step::Argument(vec![named("A"), named("B")]);
        assert_eq!(get[1].uses, [vec![both], vec![Step::Return]]);
    }

    #[test]
    fn a_cast_to_a_type_parameter_of_the_function_is_typed_by_it() {
        let src = "async function g<T>(p) { const r = await fetch(p); return (await r.json()) as T; }
                   async function h<T>(p) { const r = await fetch(p); const d = (await r.json()) as T[]; return d; }
                   async function k<T>(p) { const r = await fetch(p); return r.json<T>(); }";
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let param = |n: &str| Ty::Param(n.into());
        let json = Step::Call("json".into());
        let fetches: Vec<_> = facts.calls.iter().filter(|c| c.callee == "fetch").collect();
        let typed = |ty: Ty| vec![json.clone(), Step::Typed(ty)];
        assert!(fetches[0].uses.contains(&typed(param("T"))));
        assert!(
            fetches[1]
                .uses
                .contains(&typed(Ty::Array(Box::new(param("T")))))
        );
        assert!(fetches[2].uses.contains(&typed(param("T"))));
    }
}
