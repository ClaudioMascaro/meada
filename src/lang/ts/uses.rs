//! The shape facts of the TypeScript and JavaScript pack (spec #23): what the function making
//! each call does with its result (`Call::uses`), read from the syntax tree while it is at hand.
//! A value is followed through member and element accesses, method calls, `await`, and a
//! `const` that holds it, until it is returned or the code does something else with it.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::{BODIES, bound_nodes, named, text, unquote};
use crate::lang::Step;

/// What following values may cost in one file: every node visited, by the climb from a call or
/// by the scan of a function for a name's uses, and every step copied when a `const` is
/// followed. Past it, the file's remaining calls have no uses: they read nothing, so they give
/// no drift.
// ponytail: one budget per file; real files spend a few thousand, a crafted chain of `const`s
// would otherwise cost the square of its length.
const USES_BUDGET: usize = 1 << 20;

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
                        if named(parent).next().is_some_and(|c| c.id() == at.id()) => {}
                    "member_expression" if is("object") => {
                        let Some(property) = parent.child_by_field_name("property") else {
                            break;
                        };
                        steps.push(Step::Field(text(property, self.src).to_string()));
                    }
                    "subscript_expression" if is("object") => {
                        match parent.child_by_field_name("index") {
                            Some(i) if i.kind() == "number" => steps.push(Step::Element),
                            // A key holding `.` or `[` has no spelling in a field path.
                            Some(i) if i.kind() == "string" => match unquote(i, self.src) {
                                key if key.contains(['.', '[']) => break,
                                key => steps.push(Step::Field(key)),
                            },
                            _ => break,
                        }
                    }
                    "call_expression" if is("function") => match steps.pop() {
                        Some(Step::Field(method)) => steps.push(Step::Call(method)),
                        // The value itself is called: nothing is read from it.
                        _ => {
                            steps.clear();
                            break;
                        }
                    },
                    // What was read so far counts, and so do the uses of the name it is bound to.
                    "variable_declarator" if is("value") => {
                        if followed.insert(parent.id()) {
                            let found = self.held(parent, function);
                            if !self.charge(steps.len().saturating_mul(found.len())) {
                                return Vec::new();
                            }
                            pending.extend(found.into_iter().rev().map(|n| (n, steps.clone())));
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

    /// The uses of the name a `const` declarator binds to the value, in `function`: none when
    /// the declarator is no `const` of a plain name, or when anything else in the function
    /// binds the same name (a parameter, a `catch`, a destructuring, an inner `let`), since a
    /// use may then read another value.
    fn held(&mut self, declarator: Node<'t>, function: Node<'t>) -> Vec<Node<'t>> {
        let is_const = (declarator.parent())
            .and_then(|d| d.child_by_field_name("kind"))
            .is_some_and(|k| text(k, self.src) == "const");
        let Some(name) = declarator.child_by_field_name("name") else {
            return Vec::new();
        };
        if !is_const || name.kind() != "identifier" {
            return Vec::new();
        }
        let name = text(name, self.src);
        if !self.scopes.contains_key(&function.id()) {
            let Some(scope) = self.scan(function) else {
                return Vec::new();
            };
            self.scopes.insert(function.id(), scope);
        }
        let scope = &self.scopes[&function.id()];
        if scope.binds.get(name) != Some(&1) {
            return Vec::new();
        }
        scope.uses.get(name).cloned().unwrap_or_default()
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
    use crate::lang::{LanguagePack, Step, ts::TsJs};

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
        for key in ["\"a.b\"", "\"a[0]\"", "'items[]'"] {
            assert_eq!(read(key), [vec![json.clone()]], "{key}");
        }
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
}
