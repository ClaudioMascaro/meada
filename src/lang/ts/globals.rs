//! TypeScript runtime globals used as receivers when no binding hides their name.
//! ponytail: the listed constructors and namespaces are the ceiling for removing candidates.

use super::{Node, Parents, bound, imports, named, text};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

pub(super) fn constructor(name: &str) -> bool {
    matches!(
        name,
        "Map"
            | "Set"
            | "WeakMap"
            | "URL"
            | "URLSearchParams"
            | "Date"
            | "Error"
            | "RegExp"
            | "AbortController"
            | "Headers"
            | "Request"
            | "Response"
            | "FormData"
            | "TextEncoder"
            | "TextDecoder"
            | "Promise"
    )
}

pub(super) fn namespace(name: &str) -> bool {
    matches!(
        name,
        "JSON" | "Math" | "console" | "Object" | "Array" | "Number" | "String" | "Reflect" | "Intl"
    )
}

fn tracked(name: &str) -> bool {
    name == "fetch" || constructor(name) || namespace(name)
}

#[derive(Default)]
pub(super) struct Bindings<'t> {
    names: HashMap<String, (Vec<Node<'t>>, Vec<usize>)>,
    pub(super) imported_types: HashSet<String>,
}

impl<'t> Bindings<'t> {
    pub(super) fn visit(&mut self, node: Node<'t>, src: &[u8]) {
        let mut add = |name: &str| {
            if tracked(name) {
                self.names.entry(name.to_owned()).or_default().0.push(node);
            }
        };
        match node.kind() {
            "import_statement" => {
                let mut found = Vec::new();
                imports(node, src, &mut found);
                for import in found {
                    add(&import.local);
                    if !import.source.starts_with(['.', '/']) {
                        self.imported_types.insert(import.local);
                    }
                }
            }
            "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "class"
            | "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "internal_module"
            | "module" => {
                if let Some(name) = node.child_by_field_name("name") {
                    add(text(name, src).split('.').next().unwrap_or(""));
                }
            }
            "import_alias" | "import_require_clause" => {
                if let Some(name) = named(node).next() {
                    add(text(name, src));
                }
            }
            _ => {
                let pattern = match node.kind() {
                    "variable_declarator" => node.child_by_field_name("name"),
                    "formal_parameters" => Some(node),
                    "catch_clause" | "arrow_function" => node.child_by_field_name("parameter"),
                    "for_in_statement" => node.child_by_field_name("left"),
                    _ => None,
                };
                if let Some(pattern) = pattern {
                    for name in bound(pattern, src) {
                        add(&name);
                    }
                }
            }
        }
    }

    pub(super) fn write(&mut self, name: &str, at: usize) {
        if tracked(name) {
            self.names.entry(name.to_string()).or_default().1.push(at);
        }
    }

    pub(super) fn scopes(
        self,
        up: &Parents<'t>,
        root: Node<'t>,
        src: &[u8],
    ) -> HashMap<String, Vec<Range<usize>>> {
        self.names
            .into_iter()
            .map(|(name, (nodes, writes))| (name, up.runtime_shadows(nodes, writes, root, src)))
            .collect()
    }
}
