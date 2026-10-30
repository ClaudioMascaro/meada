//! The fields a declared type lists (spec #23, ADR 0009): what a consumer's cast, type
//! argument or annotation says the response holds. Interfaces (with `extends`), type aliases,
//! intersections, arrays and `z.infer<typeof S>` give fields; every other form adds none, and a
//! name that is not declared in the repository (a type parameter) adds none either. A type or
//! schema a package exports is an import the query fills (issue #175).

use std::collections::{BTreeMap, BTreeSet};

use crate::lang::{TYPE_DEPTH, Ty};
use crate::resolve::{Code, scope_of};
use crate::shape::{Budget, Import, Shape, Source, Spent, Want, under};
use crate::zod::Zod;

/// What reading the types of one file's consumers may cost: every type examined, over all the
/// consumers of the file. Past it, the type being read adds no fields.
// ponytail: one budget per consumer file; real types spend a few dozen, a crafted chain of
// aliases that each intersect the previous one twice would otherwise cost exponential time.
const TYPES_BUDGET: usize = 1 << 14;

/// Reads declared types, keeping the budget of each file, with the `Zod` that reads the schemas
/// a `z.infer` names: an extraction reaches zod through this one, so `ZOD_BUDGET` is spent once.
#[derive(Default)]
pub struct Types {
    zod: Zod,
    spent: Budget<TYPES_BUDGET>,
}

/// What the type arguments of a call stand for in a type written in the function it calls: the
/// function's type parameters, the arguments given and where they are written (a file and a
/// scope).
#[derive(Clone, Copy)]
pub struct Args<'a> {
    pub names: &'a [String],
    pub tys: &'a [Ty],
    pub at: (&'a str, &'a str),
}

/// The fields of a type.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Declared {
    /// Each field path (`items[].id`) with whether the type marks it optional. A path two parts
    /// of an intersection list is optional only when both mark it.
    pub fields: BTreeMap<String, bool>,
    /// The paths of the objects with an index signature: anything may be there.
    pub open: BTreeSet<String>,
    /// The types and schemas a package exports that the type names (issue #175), each at its
    /// path, for the query to fill.
    pub imports: BTreeSet<Import>,
}

impl Types {
    /// The schema reader these types share.
    pub fn zod(&mut self) -> &mut Zod {
        &mut self.zod
    }

    /// The fields `ty`, written in `path` within `scope`, lists, below the root. `None` past the
    /// file's budget.
    pub fn read<'a>(
        &mut self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        ty: &'a Ty,
    ) -> Option<Declared> {
        self.read_with(code, path, scope, ty, None)
    }

    /// The same for a type that writes the type parameters of a function, `args` standing for
    /// them: the arguments of a call of that function.
    pub fn read_with<'a>(
        &mut self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        ty: &'a Ty,
        args: Option<Args<'a>>,
    ) -> Option<Declared> {
        let mut reader = Reader {
            code,
            args,
            zod: &mut self.zod,
            spent: self.spent.of(path),
            out: Declared::default(),
            visiting: Vec::new(),
        };
        reader.walk((path, scope), ty, "", 0)?;
        Some(reader.out)
    }

    /// The shape a consumer typed with the interface or type alias `name` of `path` reads, for
    /// the package's index to store (issue #175): every path the type lists, declared, the
    /// optional ones marked, and the imports it leaves to the query. `None` when `name` is no
    /// type declaration, or past the file's budget.
    pub fn exported<'a>(&mut self, code: &Code<'a>, path: &'a str, name: &'a str) -> Option<Shape> {
        let (file, constant, ty) = code.declared_type(path, "", name)?;
        let within = scope_of(constant);
        let read = self.read(code, file, within, ty)?;
        let mut shape = Shape::new(Source::Reads(path.to_string()));
        shape.open = read.open;
        shape.imports = read.imports;
        for (field, optional) in read.fields {
            shape.list(field, optional, true);
        }
        Some(shape)
    }
}

struct Reader<'c, 'a> {
    code: &'c Code<'a>,
    args: Option<Args<'a>>,
    zod: &'c mut Zod,
    spent: Spent<'c, TYPES_BUDGET>,
    out: Declared,
    /// The declarations being followed, to stop at one that leads back to itself.
    visiting: Vec<(&'a str, &'a str)>,
}

impl<'a> Reader<'_, 'a> {
    /// Adds the fields of `ty`, written at `at` (a file and a scope), below `prefix`. `None`
    /// once the budget is spent.
    fn walk(
        &mut self,
        at: (&'a str, &'a str),
        ty: &'a Ty,
        prefix: &str,
        depth: usize,
    ) -> Option<()> {
        self.spent.charge()?;
        if depth >= TYPE_DEPTH {
            return Some(());
        }
        match ty {
            Ty::Object { members, open } => {
                if *open {
                    self.out.open.insert(prefix.to_string());
                }
                for member in members {
                    let path = under(prefix, &member.name);
                    self.field(&path, member.optional);
                    self.walk(at, &member.ty, &path, depth + 1)?;
                }
            }
            Ty::Array(of) => {
                let path = format!("{prefix}[]");
                self.field(&path, false);
                self.walk(at, of, &path, depth + 1)?;
            }
            Ty::Intersection(parts) => {
                for part in parts {
                    self.walk(at, part, prefix, depth + 1)?;
                }
            }
            Ty::Named(name) => {
                let Some((file, constant, declared)) = self.code.declared_type(at.0, at.1, name)
                else {
                    // A type a package exports is the query's to fill.
                    if let Some(imported) = self.code.imported(at.0, at.1, name) {
                        let import = Import::new(prefix, at.0, imported, Want::Type);
                        self.out.imports.insert(import);
                    }
                    return Some(());
                };
                // A declaration that leads back to itself adds what it did the first time.
                if self.visiting.contains(&(file, constant)) {
                    return Some(());
                }
                self.visiting.push((file, constant));
                let within = scope_of(constant);
                let done = self.walk((file, within), declared, prefix, depth + 1);
                self.visiting.pop();
                done?;
            }
            Ty::Schema(name) => {
                let Some((fields, imports)) = self.zod.inferred(self.code, at.0, at.1, name) else {
                    return Some(());
                };
                for (field, optional) in fields {
                    self.field(&under(prefix, &field), optional);
                }
                for import in imports {
                    let at = under(prefix, &import.at);
                    self.out.imports.insert(Import { at, ..import });
                }
            }
            Ty::Param(name) => {
                let Some(args) = self.args else {
                    return Some(());
                };
                let position = args.names.iter().position(|n| n == name);
                let Some(ty) = position.and_then(|i| args.tys.get(i)) else {
                    return Some(());
                };
                // The argument is written in the caller: its own type parameters are not ours.
                self.args = None;
                let done = self.walk(args.at, ty, prefix, depth + 1);
                self.args = Some(args);
                done?;
            }
            Ty::Unknown => {}
        }
        Some(())
    }

    /// Lists `path`; a field is optional only if every part that lists it says so.
    fn field(&mut self, path: &str, optional: bool) {
        let all = self.out.fields.entry(path.to_string()).or_insert(optional);
        *all &= optional;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Declared, Types};
    use crate::lang::{self, LanguagePack, TYPE_DEPTH, Ty, ts::TsJs};
    use crate::resolve::{Code, Resolver};
    use crate::shape::Want;

    /// The fields the type `name`, declared in `src`, lists, as `path` or `path?`, with the
    /// paths that are open.
    fn read(src: &str, name: &str) -> Option<(Vec<String>, Vec<String>)> {
        let ty = Ty::Named(name.to_string());
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let code = Code::new(&resolver, &files, lang::pack(0));
        let Declared { fields, open, .. } = Types::default().read(&code, "a.ts", "", &ty)?;
        let fields = (fields.into_iter())
            .map(|(p, optional)| format!("{p}{}", if optional { "?" } else { "" }))
            .collect();
        Some((fields, open.into_iter().collect()))
    }

    fn fields(src: &str, name: &str) -> Vec<String> {
        read(src, name).unwrap().0
    }

    #[test]
    fn interfaces_aliases_intersections_and_arrays_list_their_fields() {
        let src = "interface Base { id: string; note?: string }\n\
                   interface Row extends Base { owner: { name: string; mail?: string }; tags: string[] }\n\
                   type Rows = Array<Row>;\n\
                   type Extra = Base & { extra: number; note: string };\n\
                   interface Index { [k: string]: number; fixed: number }\n\
                   type Choice = Base | Row;\n\
                   interface Missing extends Elsewhere { own: string }\n";
        let list = |name| fields(src, name);
        assert_eq!(list("Base"), ["id", "note?"]);
        assert_eq!(
            list("Row"),
            [
                "id",
                "note?",
                "owner",
                "owner.mail?",
                "owner.name",
                "tags",
                "tags[]"
            ]
        );
        assert_eq!(
            list("Rows"),
            [
                "[]",
                "[].id",
                "[].note?",
                "[].owner",
                "[].owner.mail?",
                "[].owner.name",
                "[].tags",
                "[].tags[]"
            ]
        );
        // Optional in one part and required in the other: required.
        assert_eq!(list("Extra"), ["extra", "id", "note"]);
        assert_eq!(
            read(src, "Index").unwrap(),
            (vec!["fixed".to_string()], vec![String::new()])
        );
        assert_eq!(list("Choice"), Vec::<String>::new());
        // A type that is not declared here adds no fields; the others still do.
        assert_eq!(list("Missing"), ["own"]);
    }

    #[test]
    fn a_schema_inferred_lists_the_fields_of_its_output() {
        let src = "const S = z.object({ id: z.string(), tags: z.array(z.object({ k: z.string() })) });\n\
                   import { z } from 'zod';\n\
                   type T = z.infer<typeof S>;\n\
                   interface Holder { value: T; all: T[] }\n";
        assert_eq!(
            fields(src, "Holder"),
            [
                "all",
                "all[]",
                "all[].id",
                "all[].tags",
                "all[].tags[]",
                "all[].tags[].k",
                "value",
                "value.id",
                "value.tags",
                "value.tags[]",
                "value.tags[].k"
            ]
        );
    }

    #[test]
    fn a_schema_marks_optional_the_fields_that_may_be_absent_and_ignores_unions() {
        let src = "import { z } from 'zod';\n\
                   const O = z.object({ a: z.string(), b: z.string().optional(), c: z.string().nullish(), d: z.string().nullable(), e: z.string().default('x') });\n\
                   const P = z.object({ a: z.string(), k: z.object({ i: z.string() }) }).partial();\n\
                   const R = P.required();\n\
                   const U = z.union([z.object({ x: z.string() }), z.object({ y: z.string() })]);\n\
                   const D = z.object({ u: U, o: z.object({ k: z.string() }).or(z.object({ m: z.string() })), n: z.array(U) });\n\
                   type TO = z.infer<typeof O>;\n\
                   type TP = z.infer<typeof P>;\n\
                   type TR = z.infer<typeof R>;\n\
                   type TU = z.infer<typeof U>;\n\
                   type TD = z.infer<typeof D>;\n";
        assert_eq!(fields(src, "TO"), ["a", "b?", "c?", "d", "e"]);
        assert_eq!(fields(src, "TP"), ["a?", "k?", "k.i"]);
        assert_eq!(fields(src, "TR"), ["a", "k", "k.i"]);
        assert_eq!(fields(src, "TU"), Vec::<String>::new());
        assert_eq!(fields(src, "TD"), ["n", "n[]", "o", "u"]);
    }

    #[test]
    fn a_name_two_declarations_share_is_no_declared_type() {
        // A namespace opens no scope: both `Row` are the file's, and which one is meant is not known.
        let src = "interface Row { a: string }\n\
                   namespace N { export interface Row { b: string } }\n\
                   interface Holder { row: Row; one: One }\n\
                   interface One { c: string }\n";
        assert_eq!(fields(src, "Row"), Vec::<String>::new());
        assert_eq!(fields(src, "Holder"), ["one", "one.c", "row"]);
    }

    #[test]
    fn declarations_that_lead_back_to_themselves_end() {
        let src = "interface A extends B { a: string }\n\
                   interface B extends A { b: string }\n\
                   interface Tree { left: Tree; right: Tree; value: string }\n\
                   type Loop = Loop2;\n\
                   type Loop2 = Loop;\n";
        assert_eq!(fields(src, "A"), ["a", "b"]);
        assert_eq!(fields(src, "Tree"), ["left", "right", "value"]);
        assert_eq!(fields(src, "Loop"), Vec::<String>::new());
    }

    #[test]
    fn a_type_or_schema_a_package_exports_is_left_for_the_query() {
        let src = "import type { Ext } from '@acme/types';\n\
                   import { Schema } from '@acme/schemas';\n\
                   import { z } from 'zod';\n\
                   interface Holder { ext: Ext; list: Ext[]; inferred: z.infer<typeof Schema>; own: string }\n";
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let code = Code::new(&resolver, &files, lang::pack(0));
        let ty = Ty::Named("Holder".into());
        let read = Types::default().read(&code, "a.ts", "", &ty).unwrap();
        let imports: Vec<(&str, &str, Want)> = (read.imports.iter())
            .map(|i| (i.at.as_str(), i.name.as_str(), i.want))
            .collect();
        assert_eq!(
            imports,
            [
                ("ext", "Ext", Want::Type),
                ("inferred", "Schema", Want::Infer),
                ("list[]", "Ext", Want::Type)
            ]
        );
        // A type parameter of the same name hides the import.
        let src = "import type { Ext } from '@acme/types';\n\
                   interface Page<Ext> { item: Ext; other: Ext2 }\n\
                   import type { Ext2 } from '@acme/types';\n";
        let facts = TsJs.parse("a.ts", src.as_bytes()).unwrap();
        let files = BTreeMap::from([("a.ts", &facts)]);
        let resolver = Resolver::new(&files, TsJs.modules(&[], &files).unwrap(), &TsJs);
        let code = Code::new(&resolver, &files, lang::pack(0));
        let ty = Ty::Named("Page".into());
        let read = Types::default().read(&code, "a.ts", "", &ty).unwrap();
        let at: Vec<&str> = read.imports.iter().map(|i| i.at.as_str()).collect();
        assert_eq!(at, ["other"]);
    }

    #[test]
    fn a_name_hidden_by_a_type_parameter_is_no_declared_type() {
        let src = "interface Cursor { hidden: string }\n\
                   interface Page<Cursor> { items: Cursor[]; after: Cursor }\n";
        assert_eq!(fields(src, "Page"), ["after", "items", "items[]"]);
    }

    #[test]
    fn a_chain_of_extends_past_the_depth_limit_adds_nothing_below_it() {
        let chain = |n: usize| {
            let mut src = String::from("interface T0 { f0: string }\n");
            for i in 1..=n {
                src += &format!("interface T{i} extends T{} {{ f{i}: string }}\n", i - 1);
            }
            src
        };
        let n = TYPE_DEPTH + 8;
        let listed = fields(&chain(n), &format!("T{n}"));
        assert!(listed.contains(&format!("f{n}")));
        assert!(!listed.contains(&"f0".to_string()));
        let short = fields(
            &chain(TYPE_DEPTH / 2 - 4),
            &format!("T{}", TYPE_DEPTH / 2 - 4),
        );
        assert!(short.contains(&"f0".to_string()));
    }

    #[test]
    fn a_file_past_the_budget_gives_no_fields() {
        // Each alias intersects the previous one twice: exponential without a budget.
        let mut src = String::from("type T0 = { f: string };\n");
        for i in 1..24 {
            src += &format!("type T{i} = T{} & T{};\n", i - 1, i - 1);
        }
        assert!(read(&src, "T23").is_none());
        assert_eq!(fields(&src, "T3"), ["f"]);
    }
}
