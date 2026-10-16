//! Types that name a schema (spec #23): `z.infer<typeof S>` and the response types that wrap it.

use tree_sitter::Node;

use super::{named, text};

/// The generics whose first type argument is the type of a response body.
const RESPONSES: [&str; 2] = ["Promise", "NextResponse"];

/// The generics that give the type a schema infers.
const INFERS: [&str; 3] = ["infer", "output", "TypeOf"];

/// The schema `typed`, a type annotation or a type, names through `z.infer<typeof S>`, also
/// inside the generics a handler's return type wraps it in (`Promise<NextResponse<..>>`).
pub(super) fn inferred(typed: Node, src: &[u8]) -> Option<String> {
    let mut node = typed;
    // Each step goes one level into the type: the walk ends at the innermost one.
    loop {
        match node.kind() {
            "type_annotation" => node = named(node).next()?,
            "generic_type" => {
                let name = text(node.child_by_field_name("name")?, src);
                let name = name.rsplit('.').next().unwrap_or(name);
                let arguments = node.child_by_field_name("type_arguments")?;
                let first = named(arguments).next()?;
                if RESPONSES.contains(&name) {
                    node = first;
                } else if INFERS.contains(&name) && first.kind() == "type_query" {
                    let schema = named(first).next().filter(|s| s.kind() == "identifier")?;
                    return Some(text(schema, src).to_string());
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lang::{LanguagePack, Value, ts::TsJs};

    /// A name and the schema it is typed with.
    type Typed = Vec<(String, Option<String>)>;

    /// The schema each `const` of `src` is typed with, and each function's return type.
    fn parse(src: &str) -> (Typed, Typed) {
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let consts = (facts.consts.iter())
            .map(|c| match &c.value {
                Value::Schema(s) => (c.name.clone(), Some(s.clone())),
                _ => (c.name.clone(), None),
            })
            .collect();
        let defs = (facts.defs.iter())
            .map(|d| (d.name.clone(), d.returns.clone()))
            .collect();
        (consts, defs)
    }

    fn some(name: &str, schema: &str) -> (String, Option<String>) {
        (name.to_string(), Some(schema.to_string()))
    }

    fn none(name: &str) -> (String, Option<String>) {
        (name.to_string(), None)
    }

    #[test]
    fn a_type_that_infers_a_schema_names_it() {
        let (consts, defs) = parse(
            "const a: z.infer<typeof A> = x;\n\
             const b = y as z.output<typeof B>;\n\
             const d: Array<z.infer<typeof D>> = x;\n\
             const e: z.infer<typeof schemas.E> = x;\n\
             const f: Row = x;\n\
             function g(): z.infer<typeof G> {}\n\
             async function h(): Promise<NextResponse<z.TypeOf<typeof H>>> {}\n\
             const i = async (): Promise<z.infer<typeof I>> => ({});\n\
             function j(): Promise<number> {}\n",
        );
        assert_eq!(
            consts,
            [
                some("a", "A"),
                some("b", "B"),
                none("d"),
                none("e"),
                none("f"),
            ]
        );
        let returns: Vec<_> = defs.into_iter().collect();
        assert_eq!(
            returns,
            [some("g", "G"), some("h", "H"), some("i", "I"), none("j")]
        );
    }
}
