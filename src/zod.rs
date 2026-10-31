//! The shape of a route read from the zod schema its handler's response passes through or is
//! typed with (spec #23, ADR 0009): the source after a document and before the returned
//! literals. `z.object` gives fields, `z.array` elements, the primitives scalars, and anything
//! whose output Meada cannot read (`.transform`, `z.lazy`, `.passthrough()`, `z.any`) is open.

use std::collections::BTreeMap;

use crate::lang::{Invocation, Ty, Value};
use crate::literals::Responses;
use crate::resolve::{Code, scope_of};
use crate::shape::{Budget, Import, Shape, Source, Spent, Want};

/// What reading the schemas of one file's handlers may cost: every expression of a schema
/// examined, over all the handlers of the file. Past it, the file's remaining handlers give no
/// shape from a schema.
// ponytail: one budget per handler file; real schemas spend a few hundred, a crafted union of
// constants of unions would otherwise cost exponential time.
const ZOD_BUDGET: usize = 1 << 14;

/// How deep reading follows a schema: its nesting, the constants followed on the way and the
/// methods chained on it. A part deeper than that is open.
// ponytail: real schemas nest a handful of levels; the limit keeps a crafted chain of constants
// or methods, or objects nested thousands of levels, from costing the stack.
const ZOD_DEPTH: usize = 32;

/// The methods that leave the fields a schema gives as they are.
const KEEPS: [&str; 24] = [
    "optional",
    "nullable",
    "nullish",
    "default",
    "catch",
    "describe",
    "brand",
    "readonly",
    "strict",
    "strip",
    "refine",
    "superRefine",
    "partial",
    "required",
    "nonempty",
    "min",
    "max",
    "length",
    "int",
    "positive",
    "negative",
    "email",
    "url",
    "uuid",
];

/// The methods besides `KEEPS` that `Reader::method` reads on a schema.
const SCHEMA_METHODS: [&str; 13] = [
    "optional",
    "nullish",
    "partial",
    "required",
    "array",
    "or",
    "extend",
    "merge",
    "pick",
    "omit",
    "passthrough",
    "catchall",
    "loose",
];

/// The functions of `z` that make a scalar.
const SCALARS: [&str; 17] = [
    "string",
    "number",
    "boolean",
    "bigint",
    "date",
    "literal",
    "enum",
    "nativeEnum",
    "null",
    "undefined",
    "nan",
    "symbol",
    "email",
    "uuid",
    "url",
    "int",
    "void",
];

/// A schema's output as a tree: the fields of an object, the element of an array.
#[derive(Default)]
struct Node {
    fields: BTreeMap<String, Node>,
    element: Option<Box<Node>>,
    scalar: bool,
    /// Anything may be here besides what the tree lists.
    open: bool,
    /// The value may be absent from its object (`.optional()`, `.nullish()`, `.partial()`).
    optional: bool,
    /// A union: the parts it gives may be there or not, whichever member holds.
    mixed: bool,
    /// The schemas a package exports that give this part (issue #175), each at the root of it,
    /// for the query to fill.
    imports: Vec<Import>,
}

impl Node {
    fn scalar() -> Self {
        Self {
            scalar: true,
            ..Self::default()
        }
    }

    fn open() -> Self {
        Self {
            open: true,
            ..Self::default()
        }
    }

    /// What either of the two gives: a union.
    fn union(&mut self, other: Node) {
        for (key, node) in other.fields {
            self.fields.entry(key).or_default().union(node);
        }
        self.element = match (self.element.take(), other.element) {
            (Some(mut a), Some(b)) => {
                a.union(*b);
                Some(a)
            }
            (a, b) => a.or(b),
        };
        self.scalar |= other.scalar;
        self.open |= other.open;
        self.optional |= other.optional;
        self.mixed |= other.mixed;
        self.imports.extend(other.imports);
    }

    /// The fields of `other` added to this object, replacing those of the same name.
    fn overlay(&mut self, other: Node) {
        self.fields.extend(other.fields);
        self.open |= other.open;
        self.imports.extend(other.imports);
    }

    /// Adds the paths a consumer's `z.infer` type lists below `at` to `out`, each with whether it
    /// may be absent, and the imports it leaves to the query to `imports`. A union lists no part
    /// below it: which member holds is not known.
    fn declared(&self, at: &str, out: &mut BTreeMap<String, bool>, imports: &mut Vec<Import>) {
        if self.mixed {
            return;
        }
        for import in &self.imports {
            let at = at.to_string();
            imports.push(Import {
                at,
                want: Want::Infer,
                ..import.clone()
            });
        }
        for (key, node) in &self.fields {
            let path = match at.is_empty() {
                true => key.clone(),
                false => format!("{at}.{key}"),
            };
            out.insert(path.clone(), node.optional);
            node.declared(&path, out, imports);
        }
        if let Some(element) = &self.element {
            let path = format!("{at}[]");
            out.insert(path.clone(), false);
            element.declared(&path, out, imports);
        }
    }

    /// Adds the paths of the tree below `at` to `shape`. A part that is a scalar for one member
    /// of a union and something else for another may hold anything.
    fn render(&self, at: &str, shape: &mut Shape) {
        if !at.is_empty() {
            shape.fields.insert(at.to_string());
        }
        for import in &self.imports {
            let at = at.to_string();
            shape.imports.insert(Import {
                at,
                ..import.clone()
            });
        }
        let structured =
            !self.fields.is_empty() || self.element.is_some() || !self.imports.is_empty();
        if self.open || (self.scalar && structured) {
            shape.open.insert(at.to_string());
        } else if self.scalar {
            shape.scalars.insert(at.to_string());
        }
        for (key, node) in &self.fields {
            match at.is_empty() {
                true => node.render(key, shape),
                false => node.render(&format!("{at}.{key}"), shape),
            }
        }
        if let Some(element) = &self.element {
            element.render(&format!("{at}[]"), shape);
        }
    }
}

/// Reads handlers' schemas, keeping the budget of each file.
#[derive(Default)]
pub struct Zod {
    spent: Budget<ZOD_BUDGET>,
}

impl Zod {
    /// The shape of the 2xx responses of the handler `name` of `path` when they are typed by a
    /// zod schema: every body the result of `S.parse(..)`. The root is open when the handler
    /// may answer another way. `None` when the handler has no such schema, or past the budget.
    pub fn returned<'a>(
        &mut self,
        code: &Code<'a>,
        responses: &Responses<'a>,
        name: &str,
    ) -> Option<Shape> {
        let path = responses.path;
        let def = code.files()[path].defs.iter().find(|d| d.name == name)?;
        let scope = scope_of(&def.name);
        let mut reader = Reader {
            code,
            spent: self.spent.of(path),
        };
        let mut node = match code.returns(path, name) {
            Some(Ty::Schema(schema)) => reader.named(path, scope, schema, 0)?,
            _ => None,
        };
        if node.is_none() && !responses.bodies.is_empty() {
            let mut all: Option<Node> = None;
            for body in &responses.bodies {
                let found = reader.body(path, body.scope, body.value, 0)??;
                match &mut all {
                    Some(all) => all.union(found),
                    None => all = Some(found),
                }
            }
            node = all;
        }
        let mut shape = Shape::new(Source::Zod(path.to_string()));
        node?.render("", &mut shape);
        if responses.other {
            shape.open.insert(String::new());
        }
        Some(shape)
    }
}

impl Zod {
    /// The field paths below the root of the output of the schema `name`, written in `path`
    /// within `scope`: what `z.infer<typeof name>` lists, each with whether the output may lack
    /// it, and the schemas of a package it names (issue #175), each at its path. `None` when it is
    /// no schema Meada reads, or past the file's budget.
    pub fn inferred<'a>(
        &mut self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(BTreeMap<String, bool>, Vec<Import>)> {
        let mut reader = Reader {
            code,
            spent: self.spent.of(path),
        };
        let (mut out, mut imports) = (BTreeMap::new(), Vec::new());
        reader
            .named(path, scope, name, 0)??
            .declared("", &mut out, &mut imports);
        Some((out, imports))
    }

    /// The two shapes the package's index stores for the schema `name` of `path` (issue #175):
    /// the output a provider parsing with it sends (`Want::Zod`) and the paths a consumer typed
    /// `z.infer<typeof name>` declares (`Want::Infer`). `None` when it is no schema Meada reads,
    /// or past the file's budget.
    pub fn exported<'a>(
        &mut self,
        code: &Code<'a>,
        path: &'a str,
        name: &'a str,
    ) -> Option<(Shape, Shape)> {
        let mut reader = Reader {
            code,
            spent: self.spent.of(path),
        };
        let node = reader.named(path, "", name, 0)??;
        let mut sent = Shape::new(Source::Zod(path.to_string()));
        node.render("", &mut sent);
        let (mut out, mut imports) = (BTreeMap::new(), Vec::new());
        node.declared("", &mut out, &mut imports);
        let mut typed = Shape::new(Source::Zod(path.to_string()));
        typed.imports = imports.into_iter().collect();
        for (field, optional) in out {
            typed.list(field, optional, true);
        }
        Some((sent, typed))
    }
}

/// What reading one file's schemas gives: `Some(None)` is an expression that is no zod schema
/// (or that Meada cannot tell is one), `None` a spent budget.
type Read = Option<Option<Node>>;

struct Reader<'c, 'a> {
    code: &'c Code<'a>,
    spent: Spent<'c, ZOD_BUDGET>,
}

impl<'a> Reader<'_, 'a> {
    /// The constant `name` refers to: its file, the scope it is written in and its value.
    fn constant(
        &self,
        path: &'a str,
        scope: &'a str,
        name: &'a str,
    ) -> Option<(&'a str, &'a str, &'a Value)> {
        let ((at, constant), held) = self.code.constant(path, scope, name)?;
        Some((at, scope_of(constant), held))
    }

    /// The schema a response body is the output of: `S.parse(..)`, the `data` of
    /// `S.safeParse(..)`, a value typed `z.infer<typeof S>`, or a constant holding one.
    fn body(&mut self, path: &'a str, scope: &'a str, value: &'a Value, depth: usize) -> Read {
        self.spent.charge()?;
        if depth >= ZOD_DEPTH {
            return Some(None);
        }
        let of = |call: &'a Invocation, methods: [&str; 2]| {
            let on = (call.receiver.as_deref()).filter(|_| methods.contains(&method(call)));
            on.map(|receiver| (call, receiver))
        };
        match value {
            Value::Schema(name) => self.named(path, scope, name, depth),
            Value::Call(call) => match of(call, ["parse", "parseAsync"]) {
                Some((_, receiver)) => self.schema(path, scope, receiver, depth + 1),
                None => Some(None),
            },
            Value::Member(name, field) if field == "data" => {
                let Some((at, within, Value::Call(call))) = self.constant(path, scope, name) else {
                    return Some(None);
                };
                match of(call, ["safeParse", "safeParseAsync"]) {
                    Some((_, receiver)) => self.schema(at, within, receiver, depth + 1),
                    None => Some(None),
                }
            }
            Value::Ref(name) => match self.constant(path, scope, name) {
                Some((at, within, held)) => self.body(at, within, held, depth + 1),
                None => Some(None),
            },
            _ => Some(None),
        }
    }

    /// The schema the constant `name` holds. A name imported from outside the repository is a
    /// schema the query fills from the package that exports it (issue #175).
    fn named(&mut self, path: &'a str, scope: &'a str, name: &'a str, depth: usize) -> Read {
        if let Some((at, within, held)) = self.constant(path, scope, name) {
            return self.schema(at, within, held, depth + 1);
        }
        Some(self.code.imported(path, scope, name).map(|imported| Node {
            imports: vec![Import::new("", path, imported, Want::Zod)],
            ..Node::default()
        }))
    }

    /// The output of the schema `value` stands for.
    fn schema(&mut self, path: &'a str, scope: &'a str, value: &'a Value, depth: usize) -> Read {
        self.spent.charge()?;
        if depth >= ZOD_DEPTH {
            return Some(Some(Node::open()));
        }
        match value {
            Value::Ref(name) | Value::Schema(name) => self.named(path, scope, name, depth),
            Value::Call(call) => self.call(path, scope, call, depth),
            // A member of a namespace import of a package (`C.Quote`).
            Value::Member(head, member) => {
                let imported = self.code.imported_member(path, scope, head, member);
                Some(imported.map(|imported| Node {
                    imports: vec![Import::new("", path, imported, Want::Zod)],
                    ..Node::default()
                }))
            }
            _ => Some(None),
        }
    }

    /// The schema `value` stands for, open when it is none.
    fn part(
        &mut self,
        path: &'a str,
        scope: &'a str,
        value: &'a Value,
        depth: usize,
    ) -> Option<Node> {
        Some((self.schema(path, scope, value, depth + 1)?).unwrap_or_else(Node::open))
    }

    /// A call: a function of `z`, or a method on a schema.
    fn call(&mut self, path: &'a str, scope: &'a str, call: &'a Invocation, depth: usize) -> Read {
        if let Some(function) = self.zod(path, scope, &call.callee) {
            return self.function(path, scope, &function, &call.args, depth);
        }
        let Some(receiver) = &call.receiver else {
            return Some(None);
        };
        let Some(on) = self.schema(path, scope, receiver, depth + 1)? else {
            return Some(None);
        };
        // An imported name is taken for a schema only by the methods schemas have: `object` on
        // a `z` imported from another library makes it none.
        let imported = match &**receiver {
            Value::Ref(name) => self.constant(path, scope, name).is_none(),
            Value::Member(..) => true,
            _ => false,
        };
        if imported && !SCHEMA_METHODS.contains(&method(call)) && !KEEPS.contains(&method(call)) {
            return Some(None);
        }
        Some(Some(self.method(
            path,
            scope,
            on,
            method(call),
            &call.args,
            depth,
        )?))
    }

    /// The function of `zod` the callee names, spelled as `z` does (`object`, `coerce.string`),
    /// however it is imported.
    fn zod(&self, path: &'a str, scope: &'a str, callee: &'a str) -> Option<String> {
        let (specifier, spelled) = self.code.imported(path, scope, callee)?;
        if specifier != "zod" && !specifier.starts_with("zod/") {
            return None;
        }
        let name = (spelled.strip_prefix("z.")).or_else(|| spelled.strip_prefix("default."));
        Some(name.unwrap_or(&spelled).to_string())
    }

    /// The output of calling the function `name` of `z`.
    fn function(
        &mut self,
        path: &'a str,
        scope: &'a str,
        name: &str,
        args: &'a [Value],
        depth: usize,
    ) -> Read {
        Some(Some(match name {
            "object" | "strictObject" | "looseObject" => {
                let mut node = match args.first() {
                    Some(Value::Object(properties)) => {
                        self.object(path, scope, properties, depth)?
                    }
                    _ => Node::open(),
                };
                node.open |= name == "looseObject";
                node
            }
            "array" => {
                let element = match args.first() {
                    Some(element) => self.part(path, scope, element, depth)?,
                    None => Node::open(),
                };
                Node {
                    element: Some(Box::new(element)),
                    ..Node::default()
                }
            }
            "union" | "discriminatedUnion" => {
                let members = args.get(usize::from(name == "discriminatedUnion"));
                let members = match members {
                    Some(Value::List(members)) if !members.is_empty() => members,
                    _ => return Some(Some(Node::open())),
                };
                let mut all = Node::default();
                for member in members {
                    all.union(self.part(path, scope, member, depth)?);
                }
                all.mixed = true;
                all
            }
            name if SCALARS.contains(&name) => Node::scalar(),
            _ => Node::open(),
        }))
    }

    /// The fields of an object schema.
    fn object(
        &mut self,
        path: &'a str,
        scope: &'a str,
        properties: &'a [(String, Value)],
        depth: usize,
    ) -> Option<Node> {
        let mut node = Node::default();
        for (key, value) in properties {
            // A spread, or a key with no spelling in a field path, may be any field.
            if key == "..." || unreadable(key) {
                node.open = true;
                continue;
            }
            let field = self.part(path, scope, value, depth)?;
            node.fields.insert(key.clone(), field);
        }
        Some(node)
    }

    /// The output of calling the method `name` on a schema giving `on`.
    fn method(
        &mut self,
        path: &'a str,
        scope: &'a str,
        mut on: Node,
        name: &str,
        args: &'a [Value],
        depth: usize,
    ) -> Option<Node> {
        // The fields of an imported schema are not known here: none can be picked, left out or
        // made optional.
        if !on.imports.is_empty() && ["partial", "required", "pick", "omit"].contains(&name) {
            return Some(Node::open());
        }
        match name {
            "optional" | "nullish" => on.optional = true,
            "partial" | "required" => {
                (on.fields.values_mut()).for_each(|f| f.optional = name == "partial")
            }
            name if KEEPS.contains(&name) => {}
            "array" => {
                on = Node {
                    element: Some(Box::new(on)),
                    ..Node::default()
                }
            }
            "or" => match args.first() {
                Some(other) => {
                    on.union(self.part(path, scope, other, depth)?);
                    on.mixed = true;
                }
                None => on.open = true,
            },
            "extend" => match args.first() {
                Some(Value::Object(properties)) => {
                    on.overlay(self.object(path, scope, properties, depth)?)
                }
                _ => on.open = true,
            },
            "merge" => match args.first() {
                Some(other) => on.overlay(self.part(path, scope, other, depth)?),
                None => on.open = true,
            },
            "pick" | "omit" => match args.first() {
                Some(Value::Object(keys)) if keys.iter().all(|(k, _)| k != "...") => {
                    for _ in keys {
                        self.spent.charge()?;
                    }
                    let named = |k: &String| keys.iter().any(|(key, _)| key == k);
                    on.fields.retain(|k, _| named(k) == (name == "pick"));
                    // A computed key may name any field.
                    on.open |= keys.iter().any(|(key, _)| unreadable(key));
                }
                _ => on.open = true,
            },
            "passthrough" | "catchall" | "loose" => on.open = true,
            _ => on = Node::open(),
        }
        Some(on)
    }
}

/// Whether an object key has no spelling in a field path (a computed key, `[K]`).
fn unreadable(key: &str) -> bool {
    key.is_empty() || key.contains(['.', '['])
}

/// The method a call calls on its receiver.
fn method(call: &Invocation) -> &str {
    call.callee.rsplit('.').next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::Zod;
    use crate::lang::{self, LanguagePack, ts::TsJs};
    use crate::literals::{Handler, Literals};
    use crate::resolve::{Code, Resolver};
    use crate::shape::{Shape, Source};

    /// The shape of the handler `h` of one file `a.ts`.
    fn read(src: &str, kind: Handler) -> Option<Shape> {
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let code = Code::new(&resolver, &files, lang::pack(0));
        let responses = Literals::default().responses(&code, "a.ts", "h", kind)?;
        Zod::default().returned(&code, &responses, "h")
    }

    fn express(src: &str) -> Option<Shape> {
        read(src, Handler::Express)
    }

    fn paths(set: &BTreeSet<String>) -> Vec<&str> {
        set.iter().map(String::as_str).collect()
    }

    #[test]
    fn a_parsed_body_gives_the_fields_elements_and_scalars_of_its_schema() {
        let shape = express(
            "import { z } from 'zod';\n\
             const Row = z.object({ id: z.string(), qty: z.number().int().optional(),\n\
               tags: z.array(z.string()), owner: z.object({ name: z.string() }).nullable() });\n\
             export function h(req, res) { res.json(Row.parse(req.body)); }\n",
        )
        .unwrap();
        assert_eq!(shape.source, Source::Zod("a.ts".into()));
        let fields = ["id", "owner", "owner.name", "qty", "tags", "tags[]"];
        assert_eq!(paths(&shape.fields), fields);
        assert_eq!(paths(&shape.scalars), ["id", "owner.name", "qty", "tags[]"]);
        assert!(shape.open.is_empty());
    }

    /// The shape of `S.parse(..)` sent by an Express handler, `decls` holding the schemas.
    fn parsed(decls: &str, schema: &str) -> Shape {
        let src = format!(
            "import {{ z }} from 'zod';\n{decls}\nconst S = {schema};\n\
             export function h(req, res) {{ res.json(S.parse(req.body)); }}\n"
        );
        express(&src).unwrap()
    }

    fn fields(shape: &Shape) -> String {
        paths(&shape.fields).join(" ")
    }

    #[test]
    fn methods_change_the_field_set_as_written() {
        let base =
            "const Base = z.object({ id: z.string(), name: z.string(), secret: z.string() });";
        let cases = [
            ("Base.extend({ extra: z.number() })", "extra id name secret"),
            ("Base.omit({ secret: true })", "id name"),
            ("Base.pick({ id: true })", "id"),
            ("Base.partial()", "id name secret"),
            (
                "Base.pick({ id: true }).merge(z.object({ more: z.boolean() }))",
                "id more",
            ),
            ("z.string().array()", "[]"),
            ("Base.array().min(1)", "[] [].id [].name [].secret"),
        ];
        for (schema, expected) in cases {
            let shape = parsed(base, schema);
            assert_eq!(fields(&shape), expected, "{schema}");
            assert!(shape.open.is_empty(), "{schema}");
        }
    }

    #[test]
    fn unions_give_the_fields_of_all_their_members() {
        let cases = [
            "z.union([z.object({ a: z.string() }), z.object({ b: z.number() })])",
            "z.discriminatedUnion('k', [z.object({ a: z.string() }), z.object({ b: z.number() })])",
            "z.object({ a: z.string() }).or(z.object({ b: z.number() }))",
        ];
        for schema in cases {
            let shape = parsed("", schema);
            assert_eq!(fields(&shape), "a b", "{schema}");
            assert_eq!(paths(&shape.scalars), ["a", "b"], "{schema}");
        }
        // A part that is a scalar in one member and an object in another may hold anything.
        let shape = parsed(
            "",
            "z.union([z.object({ a: z.string() }), z.object({ a: z.object({ b: z.string() }) })])",
        );
        assert_eq!(fields(&shape), "a a.b");
        assert_eq!(paths(&shape.open), ["a"]);
    }

    #[test]
    fn what_meada_cannot_read_is_open() {
        let decls = "const Base = z.object({ id: z.string() });";
        // (schema, fields, open paths)
        let cases = [
            ("Base.passthrough()", "id", vec![""]),
            ("Base.catchall(z.string())", "id", vec![""]),
            ("z.looseObject({ a: z.string() })", "a", vec![""]),
            ("z.strictObject({ a: z.string() })", "a", vec![]),
            ("Base.strict()", "id", vec![]),
            ("z.object({ ...Base.shape, c: z.string() })", "c", vec![""]),
            ("z.object({ 'a.b': z.string() })", "", vec![""]),
            (
                "z.object({ m: z.record(z.string()), a: z.any(), u: z.unknown() })",
                "a m u",
                vec!["a", "m", "u"],
            ),
            (
                "z.object({ t: z.string().transform((s) => s.length), l: z.lazy(() => Base) })",
                "l t",
                vec!["l", "t"],
            ),
            ("z.array(z.any())", "[]", vec!["[]"]),
            ("Base.superJoin()", "", vec![""]),
            ("z.union([])", "", vec![""]),
            ("Base.pick({ [K]: true })", "", vec![""]),
            ("Base.omit({ [K]: true })", "id", vec![""]),
            ("Base.pick({ id: true, [K]: true })", "id", vec![""]),
        ];
        for (schema, fields, open) in cases {
            let shape = parsed(decls, schema);
            assert_eq!(super::tests::fields(&shape), fields, "{schema}");
            assert_eq!(paths(&shape.open), open, "{schema}");
        }
    }

    #[test]
    fn a_body_whose_schema_meada_cannot_see_gives_no_shape() {
        for decls in [
            "import { z } from 'yup';\nconst S = z.object({});",
            "const S = { parse: (x) => x };",
        ] {
            let src = format!(
                "import {{ z as zz }} from 'zod';\n{decls}\n\
                 export function h(req, res) {{ res.json(S.parse(req.body)); }}\n"
            );
            assert!(express(&src).is_none(), "{decls}");
        }
    }

    #[test]
    fn a_schema_a_package_exports_is_left_for_the_query_to_fill() {
        let decls = "import { Ext } from '@acme/schemas';";
        // (schema, fields, the paths of the imports)
        let cases = [
            ("Ext", "", vec![""]),
            (
                "z.object({ e: Ext, f: Ext.optional() })",
                "e f",
                vec!["e", "f"],
            ),
            ("z.array(Ext)", "[]", vec!["[]"]),
            ("Ext.extend({ more: z.string() })", "more", vec![""]),
            // Its fields are not known here: none can be picked or left out.
            ("Ext.pick({ id: true })", "", vec![]),
        ];
        for (schema, fields, imports) in cases {
            let shape = parsed(decls, schema);
            assert_eq!(super::tests::fields(&shape), fields, "{schema}");
            let at: Vec<&str> = shape.imports.iter().map(|i| i.at.as_str()).collect();
            assert_eq!(at, imports, "{schema}");
            let named = shape
                .imports
                .iter()
                .all(|i| i.specifier == "@acme/schemas" && i.name == "Ext" && i.from == "a.ts");
            assert!(named, "{schema}");
        }
        assert_eq!(paths(&parsed(decls, "Ext.pick({ id: true })").open), [""]);
        // A namespace import's member is one too.
        let shape = parsed("import * as C from '@acme/schemas';", "C.Ext");
        let named: Vec<&str> = shape.imports.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(named, ["Ext"]);
        // A parameter of the same name hides the import: no schema Meada reads.
        let src = "import { Ext } from '@acme/schemas';\n\
                   export function h(req, res) { [req.body].forEach((Ext) => res.json(Ext.parse(req.body))); }\n";
        assert!(express(src).is_none_or(|s| s.imports.is_empty()));
        let unhidden = src.replace("((Ext) =>", "((x) =>");
        assert_eq!(express(&unhidden).unwrap().imports.len(), 1);
    }

    const SCHEMA: &str = "import { z } from 'zod';\n\
        const S = z.object({ id: z.string(), tags: z.array(z.string()) });\n";

    fn handler(body: &str, kind: Handler) -> Option<Shape> {
        read(&format!("{SCHEMA}export {body}\n"), kind)
    }

    #[test]
    fn the_data_of_a_safe_parse_and_a_typed_value_give_the_schema() {
        let express = |body: &str| handler(body, Handler::Express);
        let bodies = [
            // The failed parse answers 400, which the shape leaves out.
            "function h(req, res) { const r = S.safeParse(req.body); if (!r.success) { \
                return res.status(400).json({ error: 1 }); } res.json(r.data); }",
            "async function h(req, res) { const r = await S.safeParseAsync(req.body); \
                const out = r.data; res.json(out); }",
            "async function h(req, res) { const out = await S.parseAsync(req.body); res.json(out); }",
            "function h(req, res) { res.json(load() as z.infer<typeof S>); }",
            "function h(req, res) { const out: z.infer<typeof S> = load(); res.send(out); }",
            "function h(req, res): z.infer<typeof S> { res.json(load()); }",
            "function h(req, res) { res.status(201).json(S.parse(req.body)); }",
        ];
        for body in bodies {
            let shape = express(body).unwrap_or_else(|| panic!("no shape for {body}"));
            assert_eq!(fields(&shape), "id tags tags[]", "{body}");
            assert!(shape.open.is_empty(), "{body}");
        }
        let next = "async function h(): Promise<NextResponse<z.infer<typeof S>>> { \
            return NextResponse.json(await load()); }";
        let shape = handler(next, Handler::Next).unwrap();
        assert_eq!(fields(&shape), "id tags tags[]");
    }

    #[test]
    fn a_handler_with_a_body_the_schema_does_not_type_gives_no_shape() {
        for body in [
            "function h(req, res) { res.json(S.parse(req.body)); res.json({ other: 1 }); }",
            "function h(req, res) { res.json(S.safeParse(req.body)); }",
            "function h(req, res) { res.json(S.parse(req.body).tags); }",
            "function h(req, res) { res.end(); }",
        ] {
            assert!(handler(body, Handler::Express).is_none(), "{body}");
        }
    }

    #[test]
    fn a_handler_that_may_answer_another_way_is_open_at_the_root() {
        let body = "function h(req, res) { if (req.q) { return res.redirect('/x'); } \
            res.json(S.parse(req.body)); }";
        let shape = handler(body, Handler::Express).unwrap();
        assert_eq!(fields(&shape), "id tags tags[]");
        assert_eq!(paths(&shape.open), [""]);
    }

    /// The shape of `S.parse(..)` for a schema nested `n` levels in each of three ways.
    fn nested(n: usize, wrap: impl Fn(usize) -> String, base: &str) -> Shape {
        let decls: String = (0..n).map(wrap).collect();
        parsed(&format!("const L0 = {base};\n{decls}"), &format!("L{n}"))
    }

    #[test]
    fn nesting_past_the_depth_limit_is_open() {
        let past = super::ZOD_DEPTH + 8;
        let leaf = "z.object({ end: z.string() })";
        // Objects inside objects, constants of constants and methods on methods.
        let shapes = [
            nested(
                past,
                |i| format!("const L{} = z.object({{ a: L{i} }});\n", i + 1),
                leaf,
            ),
            nested(past, |i| format!("const L{} = L{i};\n", i + 1), leaf),
            nested(
                past,
                |i| format!("const L{} = L{i}.optional();\n", i + 1),
                leaf,
            ),
        ];
        for (n, shape) in shapes.iter().enumerate() {
            assert!(shape.fields.len() <= super::ZOD_DEPTH, "case {n}");
            assert!(!shape.open.is_empty(), "case {n}");
            assert!(!shape.fields.contains("end"), "case {n}");
        }
        // Within the limit the leaf is read.
        let shallow = nested(
            4,
            |i| format!("const L{} = z.object({{ a: L{i} }});\n", i + 1),
            leaf,
        );
        assert!(shallow.fields.contains("a.a.a.a.end"));
        assert!(shallow.open.is_empty());
    }

    #[test]
    fn a_crafted_fan_out_of_constants_is_cut_by_the_budget() {
        // 2^40 paths through the unions if each were read in full.
        let decls: String = (0..40)
            .map(|i| format!("const L{} = z.union([L{i}, L{i}]);\n", i + 1))
            .collect();
        let src = format!(
            "import {{ z }} from 'zod';\nconst L0 = z.string();\n{decls}\
             export function h(req, res) {{ res.json(L40.parse(req.body)); }}\n"
        );
        assert!(express(&src).is_none());
    }
}
