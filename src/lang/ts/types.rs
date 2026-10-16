//! Types that name a schema (spec #23): `z.infer<typeof S>` and the response types that wrap it.

use tree_sitter::Node;

use super::{children, named, text, unquote};
use crate::lang::{Member, TYPE_DEPTH, Ty};

/// How many levels above a name a type parameter hiding it is looked for. A type parameter
/// declared farther up leaves the name unknown, never taken for a type of the file.
// ponytail: a type parameter is declared by the function or interface around its uses; the limit
// keeps a crafted nesting of thousands of levels from costing the square of its depth.
const HIDING_CLIMB: usize = 64;

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

/// The type a function's return annotation `typed` declares, inside the generics a handler wraps
/// it in (`Promise<NextResponse<..>>`); `None` when it lists nothing.
pub(super) fn returned(typed: Node, src: &[u8]) -> Option<Ty> {
    if let Some(schema) = inferred(typed, src) {
        return Some(Ty::Schema(schema));
    }
    let mut node = typed;
    // Each step goes one level into the type: the walk ends at the innermost one.
    loop {
        match node.kind() {
            "type_annotation" => node = named(node).next()?,
            "generic_type" => {
                let name = text(node.child_by_field_name("name")?, src);
                let name = name.rsplit('.').next().unwrap_or(name);
                if !RESPONSES.contains(&name) {
                    break;
                }
                node = named(node.child_by_field_name("type_arguments")?).next()?;
            }
            _ => break,
        }
    }
    Some(ty(node, src, &mut 0)).filter(|t| *t != Ty::Unknown)
}

/// The type an `interface` or `type` declaration writes: an interface is its body together with
/// the types it extends. `cost` counts the nodes read.
pub(super) fn declaration(node: Node, src: &[u8], cost: &mut usize) -> Ty {
    if node.kind() == "type_alias_declaration" {
        return (node.child_by_field_name("value")).map_or(Ty::Unknown, |v| ty(v, src, cost));
    }
    let body = (node.child_by_field_name("body")).map_or(Ty::Unknown, |b| ty(b, src, cost));
    let extends = (children(node).find(|c| c.kind() == "extends_type_clause"))
        .map(|clause| named(clause).map(|t| ty(t, src, cost)).collect::<Vec<_>>())
        .unwrap_or_default();
    match extends.is_empty() {
        true => body,
        false => Ty::Intersection([extends, vec![body]].concat()),
    }
}

/// The type `node` writes, as far as drift reads it. `cost` counts the nodes read.
pub(super) fn ty(node: Node, src: &[u8], cost: &mut usize) -> Ty {
    read(node, src, 0, cost)
}

fn read(node: Node, src: &[u8], depth: usize, cost: &mut usize) -> Ty {
    *cost += 1;
    if depth >= TYPE_DEPTH {
        return Ty::Unknown;
    }
    let first = || named(node).next();
    match node.kind() {
        "type_annotation" | "parenthesized_type" | "readonly_type" => {
            first().map_or(Ty::Unknown, |t| read(t, src, depth + 1, cost))
        }
        "array_type" => first().map_or(Ty::Unknown, |t| {
            Ty::Array(Box::new(read(t, src, depth + 1, cost)))
        }),
        "type_identifier" => reference(node, src, text(node, src), cost),
        "generic_type" => {
            if let Some(schema) = inferred(node, src) {
                return Ty::Schema(schema);
            }
            let Some(name) = node.child_by_field_name("name") else {
                return Ty::Unknown;
            };
            let argument =
                (node.child_by_field_name("type_arguments")).and_then(|a| named(a).next());
            match (name.kind(), text(name, src), argument) {
                ("type_identifier", "Array" | "ReadonlyArray", Some(of)) => {
                    Ty::Array(Box::new(read(of, src, depth + 1, cost)))
                }
                ("type_identifier", name, _) => reference(node, src, name, cost),
                _ => Ty::Unknown,
            }
        }
        "intersection_type" => {
            // `A & B & C` nests to the left: flattened with a stack, not by recursion.
            let (mut parts, mut stack) = (Vec::new(), vec![node]);
            while let Some(at) = stack.pop() {
                *cost += 1;
                match at.kind() {
                    "intersection_type" => {
                        stack.extend(named(at).collect::<Vec<_>>().into_iter().rev())
                    }
                    _ => parts.push(read(at, src, depth + 1, cost)),
                }
            }
            Ty::Intersection(parts)
        }
        "object_type" | "interface_body" => {
            let mut members = Vec::new();
            let mut open = false;
            for member in named(node) {
                *cost += 1;
                match member.kind() {
                    "index_signature" => open = true,
                    "property_signature" => members.extend(property(member, src, depth, cost)),
                    _ => {}
                }
            }
            Ty::Object { members, open }
        }
        _ => Ty::Unknown,
    }
}

/// A property of an object type, unless its name has no spelling in a field path.
fn property(node: Node, src: &[u8], depth: usize, cost: &mut usize) -> Option<Member> {
    let key = node.child_by_field_name("name")?;
    let name = match key.kind() {
        "property_identifier" => text(key, src).to_string(),
        "string" => unquote(key, src),
        _ => return None,
    };
    if name.is_empty() || name.contains(['.', '[']) {
        return None;
    }
    Some(Member {
        name,
        optional: children(node).any(|c| c.kind() == "?"),
        ty: (node.child_by_field_name("type"))
            .map_or(Ty::Unknown, |t| read(t, src, depth + 1, cost)),
    })
}

/// The name `name`, used at `node`, unless a type parameter of a function, class, interface or
/// alias around it hides it.
fn reference(node: Node, src: &[u8], name: &str, cost: &mut usize) -> Ty {
    let mut at = node;
    for _ in 0..HIDING_CLIMB {
        *cost += 1;
        let Some(parent) = at.parent() else {
            return Ty::Named(name.to_string());
        };
        let declares = (parent.child_by_field_name("type_parameters")).is_some_and(|ps| {
            named(ps).any(|p| {
                p.child_by_field_name("name")
                    .is_some_and(|n| text(n, src) == name)
            })
        });
        if declares {
            return Ty::Unknown;
        }
        at = parent;
    }
    Ty::Unknown
}

#[cfg(test)]
mod tests {
    use super::HIDING_CLIMB;
    use crate::lang::{LanguagePack, TYPE_DEPTH, Value, ts::TsJs};

    /// A name and the schema it is typed with.
    type Typed = Vec<(String, Option<String>)>;

    fn schema(ty: &Option<Ty>) -> Option<String> {
        match ty {
            Some(Ty::Schema(s)) => Some(s.clone()),
            _ => None,
        }
    }

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
            .map(|d| (d.name.clone(), schema(&d.returns)))
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

    use crate::lang::{Member, Ty};

    /// The type each `interface` and `type` of `src` declares.
    fn declared(src: &str) -> Vec<(String, Ty)> {
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        (facts.consts.into_iter())
            .filter_map(|c| match c.value {
                Value::Type(ty) => Some((c.name, ty)),
                _ => None,
            })
            .collect()
    }

    fn named(name: &str) -> Ty {
        Ty::Named(name.into())
    }

    fn member(name: &str, optional: bool, ty: Ty) -> Member {
        Member {
            name: name.into(),
            optional,
            ty,
        }
    }

    fn object(members: Vec<Member>) -> Ty {
        Ty::Object {
            members,
            open: false,
        }
    }

    #[test]
    fn declarations_list_the_members_of_their_types() {
        let types = declared(
            "interface A extends B, C<X> { a: string; b?: D[]; \"q\": Array<E>; m(): void; n: F & { x: 1 }; o: G | null }\n\
             type R = A & { z: number };\n\
             type U = A | B;\n\
             export interface S { [k: string]: number; k: Row }\n\
             type M = readonly (Row)[];\n",
        );
        let a = Ty::Intersection(vec![
            named("B"),
            named("C"),
            object(vec![
                member("a", false, Ty::Unknown),
                member("b", true, Ty::Array(Box::new(named("D")))),
                member("q", false, Ty::Array(Box::new(named("E")))),
                member(
                    "n",
                    false,
                    Ty::Intersection(vec![
                        named("F"),
                        object(vec![member("x", false, Ty::Unknown)]),
                    ]),
                ),
                member("o", false, Ty::Unknown),
            ]),
        ]);
        let r = Ty::Intersection(vec![
            named("A"),
            object(vec![member("z", false, Ty::Unknown)]),
        ]);
        let s = Ty::Object {
            members: vec![member("k", false, named("Row"))],
            open: true,
        };
        let m = Ty::Array(Box::new(named("Row")));
        let want = [("A", a), ("R", r), ("U", Ty::Unknown), ("S", s), ("M", m)];
        let want: Vec<_> = want.into_iter().map(|(n, t)| (n.to_string(), t)).collect();
        assert_eq!(types, want);
    }

    #[test]
    fn a_name_used_past_the_hiding_climb_is_unknown() {
        let used = |blocks: usize| {
            let src = format!(
                "{}type T = {{ c: Cursor }};{}",
                "{ ".repeat(blocks),
                " }".repeat(blocks)
            );
            declared(&src).remove(0).1
        };
        assert_eq!(used(3), object(vec![member("c", false, named("Cursor"))]));
        assert_eq!(
            used(HIDING_CLIMB + 6),
            object(vec![member("c", false, Ty::Unknown)])
        );
    }

    #[test]
    fn a_type_nested_past_the_depth_limit_is_unknown_from_there() {
        let nested = |n: usize| {
            let src = format!(
                "type T = {}{{ c: string }}{};",
                "{ a: ".repeat(n),
                " }".repeat(n)
            );
            let mut ty = declared(&src).remove(0).1;
            let mut levels = 0;
            while let Ty::Object { mut members, .. } = ty {
                levels += 1;
                ty = members.remove(0).ty;
            }
            levels
        };
        assert_eq!(nested(4), 5);
        let deep = nested(10 * TYPE_DEPTH);
        assert!((TYPE_DEPTH / 2..=TYPE_DEPTH).contains(&deep), "{deep}");
    }

    #[test]
    fn a_type_parameter_hides_the_type_of_its_name() {
        let types = declared(
            "interface Page<T> { items: T[]; next: Cursor }\n\
             type Box<Cursor> = { v: Cursor };\n",
        );
        let page = object(vec![
            member("items", false, Ty::Array(Box::new(Ty::Unknown))),
            member("next", false, named("Cursor")),
        ]);
        let boxed = object(vec![member("v", false, Ty::Unknown)]);
        assert_eq!(
            types,
            [("Page".to_string(), page), ("Box".to_string(), boxed)]
        );
    }
}
