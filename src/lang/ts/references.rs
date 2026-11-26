//! The non-call uses of names in the TypeScript and JavaScript pack (`Facts::references`): a
//! name rendered as a JSX element, written as a type, read as a value or a member, extended or
//! implemented, re-exported, or taken off a dynamic `import()`. One per scope, name and kind
//! (and specifier, for a re-export or a dynamic one), the first occurrence's range kept. The
//! pack records them whatever binds the name (a local, a parameter, a global): resolution drops
//! what binds no declaration.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use tree_sitter::Node;

use super::{Parents, callee, children, dynamic_import, named, text, unquote, value};
use crate::lang::{Reference, Use};

/// The references of one file, as the walk visits its nodes.
#[derive(Default)]
pub(super) struct References<'t> {
    seen: HashSet<(String, String, Use, Option<String>)>,
    found: Vec<Reference>,
    /// The type parameters (and `infer` and mapped names) declared so far, by name, with the
    /// range of the node they are in scope in: a type use inside it names no declaration.
    params: HashMap<&'t str, Vec<Range<usize>>>,
}

impl<'t> References<'t> {
    pub(super) fn into_vec(self) -> Vec<Reference> {
        self.found
    }

    fn push(
        &mut self,
        scope: &str,
        name: String,
        kind: Use,
        range: Range<usize>,
        from: Option<String>,
    ) {
        // A name a type parameter or a block's own type declares there is no file-level name.
        let head = name.split('.').next().unwrap_or(&name);
        let hidden =
            (self.params.get(head)).is_some_and(|rs| rs.iter().any(|r| r.contains(&range.start)));
        if from.is_none() && hidden {
            return;
        }
        if self
            .seen
            .insert((scope.to_string(), name.clone(), kind, from.clone()))
        {
            self.found.push(Reference {
                scope: scope.to_string(),
                name,
                kind,
                range,
                from,
            });
        }
    }

    /// The references `node` makes, visited at `scope`.
    pub(super) fn visit(&mut self, node: Node<'t>, scope: &str, src: &'t [u8], up: &Parents<'t>) {
        let parent = up.of(node);
        let field_of = |p: Node, field: &str| {
            p.child_by_field_name(field)
                .is_some_and(|c| c.id() == node.id())
        };
        match node.kind() {
            "jsx_opening_element" | "jsx_self_closing_element" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let jsx = match name.kind() {
                        "identifier"
                            if text(name, src).starts_with(|c: char| c.is_ascii_uppercase()) =>
                        {
                            Some(text(name, src).to_string())
                        }
                        "member_expression" | "nested_identifier" => dotted(name, src),
                        _ => None,
                    };
                    if let Some(jsx) = jsx {
                        self.push(scope, jsx, Use::Jsx, name.byte_range(), None);
                    }
                }
            }
            "type_parameter" => {
                if let (Some(name), Some(owner)) = (
                    node.child_by_field_name("name"),
                    parent.and_then(|p| up.of(p)),
                ) {
                    self.declare(text(name, src), owner.byte_range());
                }
            }
            "infer_type" => {
                let owner = std::iter::successors(parent, |p| up.of(*p))
                    .find(|p| p.kind() == "conditional_type");
                if let (Some(name), Some(owner)) =
                    (named(node).find(|c| c.kind() == "type_identifier"), owner)
                {
                    self.declare(text(name, src), owner.byte_range());
                }
            }
            // An interface, type or enum declared inside a block is the block's: no symbol, and
            // it hides a file-level one of its name there. (A use before it in the block slips.)
            "interface_declaration" | "type_alias_declaration" | "enum_declaration"
                if parent.is_some_and(|p| !matches!(p.kind(), "program" | "export_statement")) =>
            {
                if let (Some(name), Some(owner)) = (node.child_by_field_name("name"), parent) {
                    self.declare(text(name, src), owner.byte_range());
                }
            }
            "mapped_type_clause" => {
                if let (Some(name), Some(owner)) = (node.child_by_field_name("name"), parent) {
                    self.declare(text(name, src), owner.byte_range());
                }
            }
            "type_identifier" | "nested_type_identifier" => {
                let Some(p) = parent else { return };
                // A declared name, a part of a dotted one, or a type parameter's own.
                if p.kind() == "nested_type_identifier"
                    || field_of(p, "name") && p.kind() != "generic_type"
                    || matches!(p.kind(), "type_parameter" | "infer_type")
                {
                    return;
                }
                let name = match node.kind() {
                    "type_identifier" => text(node, src).to_string(),
                    _ => match dotted(node, src) {
                        Some(name) => name,
                        None => return,
                    },
                };
                let head = name.split('.').next().unwrap_or(&name);
                if self
                    .params
                    .get(head)
                    .is_some_and(|rs| rs.iter().any(|r| r.contains(&node.start_byte())))
                {
                    return;
                }
                let heritage = std::iter::successors(Some(p), |p| up.of(*p))
                    .take(2)
                    .any(|p| matches!(p.kind(), "implements_clause" | "extends_type_clause"));
                let kind = if heritage { Use::Extends } else { Use::Type };
                self.push(scope, name, kind, node.byte_range(), None);
            }
            "export_statement" => {
                // A re-export keeps its source: `export *` of two modules are two uses.
                let Some(source) = node.child_by_field_name("source") else {
                    return;
                };
                let from = Some(unquote(source, src));
                for child in named(node) {
                    match child.kind() {
                        "export_clause" => {
                            for spec in named(child).filter(|c| c.kind() == "export_specifier") {
                                if let Some(name) = spec.child_by_field_name("name") {
                                    let name = text(name, src).to_string();
                                    self.push(
                                        "",
                                        name,
                                        Use::Reexport,
                                        spec.byte_range(),
                                        from.clone(),
                                    );
                                }
                            }
                        }
                        // `export * as ns from`.
                        "namespace_export" => self.push(
                            "",
                            "*".into(),
                            Use::Reexport,
                            child.byte_range(),
                            from.clone(),
                        ),
                        _ => {}
                    }
                }
                if children(node).any(|c| c.kind() == "*") {
                    self.push("", "*".into(), Use::Reexport, node.byte_range(), from);
                }
            }
            "member_expression" => self.member(node, parent, scope, src, up),
            "identifier" | "shorthand_property_identifier" => self.read(node, parent, scope, src),
            "call_expression" => self.dynamic(node, scope, src, up),
            "variable_declarator" => {
                // `const { X, Y: z } = await import('x')`.
                let (Some(pattern), Some(from)) = (
                    node.child_by_field_name("name")
                        .filter(|n| n.kind() == "object_pattern"),
                    node.child_by_field_name("value")
                        .filter(|v| v.kind() == "await_expression")
                        .and_then(|v| named(v).next())
                        .and_then(|c| imported(c, src, scope, up)),
                ) else {
                    return;
                };
                for key in named(pattern) {
                    let name = match key.kind() {
                        "shorthand_property_identifier_pattern" => Some(key),
                        "pair_pattern" => key
                            .child_by_field_name("key")
                            .filter(|k| k.kind() == "property_identifier"),
                        "object_assignment_pattern" => key.child_by_field_name("left"),
                        _ => None,
                    };
                    if let Some(name) = name {
                        self.push(
                            scope,
                            text(name, src).to_string(),
                            Use::Dynamic,
                            key.byte_range(),
                            Some(from.clone()),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn declare(&mut self, name: &'t str, range: Range<usize>) {
        self.params.entry(name).or_default().push(range);
    }

    /// A member read: its dotted name, or, as the callee of a call, a member of its head alone;
    /// `(await import('x')).X` a dynamic use of `X`.
    fn member(
        &mut self,
        node: Node<'t>,
        parent: Option<Node<'t>>,
        scope: &str,
        src: &'t [u8],
        up: &Parents<'t>,
    ) {
        let Some(p) = parent else { return };
        let is = |field: &str| {
            p.child_by_field_name(field)
                .is_some_and(|c| c.id() == node.id())
        };
        if let Some(object) = node.child_by_field_name("object")
            && object.kind() == "parenthesized_expression"
            && let Some(awaited) = named(object)
                .next()
                .filter(|n| n.kind() == "await_expression")
            && let Some(from) = named(awaited)
                .next()
                .and_then(|c| imported(c, src, scope, up))
            && let Some(property) = node.child_by_field_name("property")
        {
            self.push(
                scope,
                text(property, src).to_string(),
                Use::Dynamic,
                node.byte_range(),
                Some(from),
            );
            return;
        }
        // The outer member of a chain, the name of a JSX element and a written member say nothing.
        if p.kind() == "member_expression" && is("object")
            || matches!(
                p.kind(),
                "jsx_opening_element" | "jsx_self_closing_element" | "jsx_closing_element"
            )
            || matches!(
                p.kind(),
                "assignment_expression" | "augmented_assignment_expression"
            ) && is("left")
        {
            return;
        }
        let called = matches!(p.kind(), "call_expression" | "new_expression")
            && (is("function") || is("constructor"));
        let Some(name) = dotted(node, src) else {
            return;
        };
        let heritage = matches!(p.kind(), "extends_clause" | "class_heritage");
        let (name, kind) = match (called, heritage) {
            (true, _) => (
                name.split('.').next().unwrap_or(&name).to_string(),
                Use::Member,
            ),
            (false, true) => (name, Use::Extends),
            (false, false) if p.kind() == "type_query" => (name, Use::Type),
            (false, false) => (name, Use::Member),
        };
        self.push(scope, name, kind, node.byte_range(), None);
    }

    /// An identifier read in expression position.
    fn read(&mut self, node: Node<'t>, parent: Option<Node<'t>>, scope: &str, src: &'t [u8]) {
        let Some(p) = parent else { return };
        let is = |field: &str| {
            p.child_by_field_name(field)
                .is_some_and(|c| c.id() == node.id())
        };
        let name = text(node, src);
        let kind = match p.kind() {
            "extends_clause" | "class_heritage" => Use::Extends,
            "type_query" => Use::Type,
            // A callee, a member's object (the member is the use), a JSX element's name.
            "call_expression" | "new_expression" if is("function") || is("constructor") => return,
            "member_expression" if is("object") => return,
            "jsx_opening_element" | "jsx_self_closing_element" | "jsx_closing_element" => return,
            // What declares or binds a name, and what writes one.
            "variable_declarator"
            | "function_declaration"
            | "function_expression"
            | "generator_function_declaration"
            | "generator_function"
            | "class_declaration"
            | "class"
            | "abstract_class_declaration"
            | "enum_declaration"
            | "function_signature"
            | "internal_module"
            | "module"
            | "method_definition"
            | "required_parameter"
            | "optional_parameter"
            | "labeled_statement"
                if is("name") || is("pattern") || is("label") =>
            {
                return;
            }
            "arrow_function" if is("parameter") => return,
            "catch_clause" if is("parameter") => return,
            "assignment_expression" | "augmented_assignment_expression" | "for_in_statement"
                if is("left") =>
            {
                return;
            }
            "assignment_pattern" | "object_assignment_pattern" if is("left") => return,
            "pair_pattern" if is("value") => return,
            "update_expression" => return,
            "formal_parameters"
            | "array_pattern"
            | "rest_pattern"
            | "import_clause"
            | "namespace_import"
            | "import_specifier"
            | "export_specifier"
            | "namespace_export"
            | "import_alias"
            | "import_require_clause"
            // A part of a dotted type or namespace name (the whole is the use).
            | "nested_type_identifier"
            | "nested_identifier"
            | "jsx_namespace_name" => return,
            _ => Use::Value,
        };
        self.push(scope, name.to_string(), kind, node.byte_range(), None);
    }

    /// `import('x').then(m => m.X)` and `.then(m => ({ default: m.X }))`: a dynamic use of each
    /// member read off `m`, the callback's first parameter.
    fn dynamic(&mut self, node: Node<'t>, scope: &str, src: &'t [u8], up: &Parents<'t>) {
        let Some(function) = node
            .child_by_field_name("function")
            .filter(|f| f.kind() == "member_expression")
        else {
            return;
        };
        let (Some(object), Some(property)) = (
            function.child_by_field_name("object"),
            function.child_by_field_name("property"),
        ) else {
            return;
        };
        let Some(from) = (text(property, src) == "then")
            .then(|| imported(object, src, scope, up))
            .flatten()
        else {
            return;
        };
        let Some(callback) = node
            .child_by_field_name("arguments")
            .and_then(|a| named(a).next())
            .filter(|c| c.kind() == "arrow_function" || c.kind() == "function_expression")
        else {
            return;
        };
        let param = callback.child_by_field_name("parameter").or_else(|| {
            let params = callback.child_by_field_name("parameters")?;
            let first = named(params).next()?;
            match first.kind() {
                "identifier" => Some(first),
                _ => first
                    .child_by_field_name("pattern")
                    .filter(|p| p.kind() == "identifier"),
            }
        });
        let (Some(param), Some(body)) = (param, callback.child_by_field_name("body")) else {
            return;
        };
        let m = text(param, src);
        // The members read off `m` in the callback's body; explicit stack, as the walk.
        let mut stack = vec![body];
        while let Some(n) = stack.pop() {
            if n.kind() == "member_expression"
                && let (Some(o), Some(p)) = (
                    n.child_by_field_name("object"),
                    n.child_by_field_name("property"),
                )
                && o.kind() == "identifier"
                && text(o, src) == m
            {
                self.push(
                    scope,
                    text(p, src).to_string(),
                    Use::Dynamic,
                    n.byte_range(),
                    Some(from.clone()),
                );
            }
            stack.extend(named(n));
        }
    }
}

/// The specifier of `import('<literal>')`, when `node` is that call and the literal has no
/// template hole (`dynamic_import` decides, on the call's value).
fn imported(node: Node, src: &[u8], scope: &str, up: &Parents) -> Option<String> {
    let function = node
        .child_by_field_name("function")
        .filter(|_| node.kind() == "call_expression")?;
    if function.kind() != "import" {
        return None;
    }
    dynamic_import(&value(node, src, scope, up)).map(str::to_string)
}

/// The dotted name of a member chain whose head is an identifier (`config.url`, `Ns.Comp`);
/// `None` for one on `this`, a call or any other expression.
fn dotted(node: Node, src: &[u8]) -> Option<String> {
    let name = match node.kind() {
        "nested_identifier" | "nested_type_identifier" => {
            text(node, src).split_whitespace().collect()
        }
        _ => callee(node, src),
    };
    let head = name.split('.').next()?;
    (!head.is_empty() && !head.starts_with('(') && !matches!(head, "this" | "super"))
        .then_some(name)
}
