//! Shapes (spec #21, ADR 0009): what a contract carries in its payload, as a set of field paths,
//! in one form whatever it was read from, so shape drift (spec #23) compares shapes without
//! caring about their source.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::contract::Level;

/// The field paths a payload carries. A path is dotted, with array elements written `[]`
/// (`items[].id`); a payload that is itself an array starts with `[]` (`[].id`). Every node
/// below the root is a field, so `items` and `items[]` are listed with `items[].id`. What is
/// recorded is presence, the scalar leaves and a consumer's optional fields; types are never
/// compared, and no format is kept.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    pub source: Source,
    pub fields: BTreeSet<String>,
    /// The parts whose fields Meada cannot list, by path; the empty path is the whole payload.
    /// Anything below an open path may be present.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub open: BTreeSet<String>,
    /// The fields known to be scalars (a string, a number, a boolean), by path; the empty path
    /// is a payload that is itself one. Nothing lies below a scalar, so drift checks a consumer
    /// path only up to it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub scalars: BTreeSet<String>,
    /// For a consumer, the paths a declared type gives rather than a field read (spec #23):
    /// drift on one of them is `exact` when the provider's shape is declared too.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub declared: BTreeSet<String>,
    /// For a consumer, the declared paths its type marks optional (`name?: string`) that no field
    /// read makes certain, by path: drift does not report one the provider lacks, nor what lies
    /// below it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub optional: BTreeSet<String>,
    /// The parts a name imported from a package gives (issue #175), left for the query to fill
    /// from the shape the package's index stores for that name (`fill`): stored, never exported.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub imports: BTreeSet<Import>,
    /// For a provider, the parts a package import filled through a resolution that was not
    /// `exact` (issue #175: an `outDir` mapping that may have picked the wrong file), by path: a
    /// consumer path missing at or below one is at most `inferred`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub guessed: BTreeSet<String>,
}

impl Shape {
    /// A shape with no fields, read from `source`.
    pub fn new(source: Source) -> Self {
        Self {
            source,
            fields: BTreeSet::new(),
            open: BTreeSet::new(),
            scalars: BTreeSet::new(),
            declared: BTreeSet::new(),
            optional: BTreeSet::new(),
            imports: BTreeSet::new(),
            guessed: BTreeSet::new(),
        }
    }

    /// Lists `path`: read or required (`optional` false), or marked optional by the type that
    /// gives it, and `declared` when that type counts as declared. The one place a field joins
    /// `optional` or `declared`, so both stay within `fields`, and a path is optional only while
    /// everything listing it says so: a read or a type requiring it makes it certain, in any
    /// order.
    pub fn list(&mut self, path: String, optional: bool, declared: bool) {
        let optional = optional && (!self.fields.contains(&path) || self.optional.contains(&path));
        match optional {
            true => self.optional.insert(path.clone()),
            false => self.optional.remove(&path),
        };
        if declared {
            self.declared.insert(path.clone());
        }
        self.fields.insert(path);
    }

    /// Adds the paths of `other`, read from the same side, to this shape.
    pub fn merge(&mut self, other: &Shape) {
        self.open.extend(other.open.iter().cloned());
        self.scalars.extend(other.scalars.iter().cloned());
        for p in &other.fields {
            let (optional, declared) = (other.optional.contains(p), other.declared.contains(p));
            self.list(p.clone(), optional, declared);
        }
        self.imports.extend(other.imports.iter().cloned());
        self.guessed.extend(other.guessed.iter().cloned());
    }

    /// Puts `stored`, the shape the package's index holds for the name `import` imports, at
    /// the import's path, in the view its `want` reads: a provider's fields, open parts and
    /// scalars, the open parts alone of a type a provider's value is typed with, or a
    /// consumer's declared paths (optional only when every type listing a path says so, and
    /// never when it is read). `level` is how the import resolved: through one that is not
    /// `exact`, a provider's part is guessed and a consumer's paths are not declared, so no
    /// drift on them is `exact`. Without a stored shape, a provider's part is open, and a
    /// type's or a consumer's adds nothing.
    fn graft(&mut self, import: &Import, stored: Option<(&Shape, Level)>) {
        let at = import.at.as_str();
        if import.want == Want::Opens {
            for p in stored.into_iter().flat_map(|(s, _)| &s.open) {
                let q = under(at, p);
                if !q.is_empty() {
                    self.fields.insert(q.clone());
                }
                self.open.insert(q);
            }
            return;
        }
        if !import.want.declares() {
            if !at.is_empty() {
                self.fields.insert(at.to_string());
            }
            let Some((s, level)) = stored else {
                self.open.insert(at.to_string());
                return;
            };
            self.fields.extend(s.fields.iter().map(|p| under(at, p)));
            self.open.extend(s.open.iter().map(|p| under(at, p)));
            self.scalars.extend(s.scalars.iter().map(|p| under(at, p)));
            self.guessed.extend(s.guessed.iter().map(|p| under(at, p)));
            if level != Level::Exact {
                self.guessed.insert(at.to_string());
            }
            return;
        }
        let Some((s, level)) = stored else {
            return;
        };
        for p in &s.declared {
            self.list(under(at, p), s.optional.contains(p), level == Level::Exact);
        }
    }
}

/// Which view of an exported name a part imported from a package needs (issue #175), so which
/// of the shapes the package's index stores for the name fills it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Want {
    /// The output of a zod schema, on a provider (`S.parse(..)`).
    Zod,
    /// The fields `z.infer<typeof S>` lists, on a consumer.
    Infer,
    /// An object literal constant, on a provider (`{ ...DEFAULTS }`).
    Literals,
    /// The fields an interface or a type alias lists, on a consumer.
    Type,
    /// The open parts (index signatures) of an interface or a type alias that a provider's
    /// value is typed with: the `Type` shape, of which a provider takes only those.
    Opens,
}

impl Want {
    const ALL: [Want; 4] = [Want::Zod, Want::Infer, Want::Literals, Want::Type];

    /// The view of the shape the package keeps that this one reads.
    pub fn kept(self) -> Want {
        match self {
            Want::Opens => Want::Type,
            want => want,
        }
    }

    /// The name the index stores.
    pub fn name(self) -> &'static str {
        match self {
            Want::Zod => "zod",
            Want::Infer => "infer",
            Want::Literals => "literals",
            Want::Type => "type",
            Want::Opens => "opens",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.name() == name)
    }

    /// Whether the view is a consumer's declared paths rather than a provider's fields.
    fn declares(self) -> bool {
        matches!(self, Want::Infer | Want::Type)
    }
}

/// A part of a shape that a name imported from outside the repository gives (issue #175): its
/// path in the shape, the file whose import names it, the specifier and the name spelled from
/// it (`Quote`, `default`), and the view it needs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Import {
    pub at: String,
    pub from: String,
    pub specifier: String,
    pub name: String,
    pub want: Want,
}

impl Import {
    /// The import `imported` (`Code::imported`'s specifier and name) written in `from`, at `at`.
    pub fn new(at: &str, from: &str, (specifier, name): (&str, String), want: Want) -> Self {
        Self {
            at: at.to_string(),
            from: from.to_string(),
            specifier: specifier.to_string(),
            name,
            want,
        }
    }
}

/// How many packages a shape is followed through, one import inside the shape of another. A
/// part deeper than that is unresolved: open on a provider, nothing on a consumer.
// ponytail: real contracts share one package, maybe two; the limit keeps a crafted ring of
// packages that import each other's shapes from following it forever.
pub const PACKAGE_HOPS: usize = 8;

/// What filling the imports of one side may cost: every import looked up and every path it
/// grafts. Past it, the side's remaining imports are unresolved.
// ponytail: one budget per side; a crafted package whose shapes each import a hundred others
// would otherwise multiply its paths at every hop.
pub const GRAFT_BUDGET: usize = 1 << 16;

/// `shape` with each of its imports filled (issue #175): `find` gives the shape the package's
/// index stores for the imported name, with where that shape is (`R`, the repository its own
/// imports resolve from) and the level the import resolved at, and that shape is filled the
/// same way before it is grafted (the types inside a type a provider is typed with giving their
/// open parts too), at most
/// `PACKAGE_HOPS` deep and within `GRAFT_BUDGET`. An import `find` cannot give, or past either
/// limit, is unresolved. The result has no imports left.
pub fn fill<'s, R: Copy>(
    shape: &Shape,
    at: R,
    find: &mut impl FnMut(R, &Import) -> Option<(R, &'s Shape, Level)>,
) -> Shape {
    filled(shape, at, 0, &mut 0, find)
}

fn filled<'s, R: Copy>(
    shape: &Shape,
    at: R,
    depth: usize,
    spent: &mut usize,
    find: &mut impl FnMut(R, &Import) -> Option<(R, &'s Shape, Level)>,
) -> Shape {
    let mut out = Shape {
        imports: BTreeSet::new(),
        ..shape.clone()
    };
    for import in &shape.imports {
        *spent += 1;
        let found = (depth < PACKAGE_HOPS && *spent <= GRAFT_BUDGET)
            .then(|| find(at, import))
            .flatten();
        let stored = found.map(|(r, s, level)| {
            let opens;
            let s = match import.want {
                Want::Opens => {
                    opens = Shape {
                        imports: (s.imports.iter())
                            .filter(|i| i.want == Want::Type)
                            .map(|i| Import {
                                want: Want::Opens,
                                ..i.clone()
                            })
                            .collect(),
                        ..s.clone()
                    };
                    &opens
                }
                _ => s,
            };
            (filled(s, r, depth + 1, spent, find), level)
        });
        let stored = stored.filter(|(s, _)| {
            *spent += s.fields.len() + s.declared.len();
            *spent <= GRAFT_BUDGET
        });
        out.graft(import, stored.as_ref().map(|(s, level)| (s, *level)));
    }
    out
}

/// What one extraction has spent, by file, of a shape reader's per-file budget `LIMIT`
/// (`ZOD_BUDGET`, `TYPES_BUDGET`, `LITERALS_BUDGET`). An extraction holds one reader of each
/// source (`Literals` holds its `Types`, which hold their `Zod`), so each budget is spent at
/// most once per file.
#[derive(Default)]
pub struct Budget<const LIMIT: usize>(HashMap<String, usize>);

impl<const LIMIT: usize> Budget<LIMIT> {
    /// What the file `path` has spent, to charge.
    pub fn of(&mut self, path: &str) -> Spent<'_, LIMIT> {
        Spent(self.0.entry(path.to_string()).or_default())
    }
}

/// What one file has spent of a `Budget`.
pub struct Spent<'c, const LIMIT: usize>(&'c mut usize);

impl<const LIMIT: usize> Spent<'_, LIMIT> {
    /// Charges one unit; `None` once the file's budget is spent.
    pub fn charge(&mut self) -> Option<()> {
        *self.0 += 1;
        (*self.0 <= LIMIT).then_some(())
    }
}

/// The path `path`, written from the root of a part found at `prefix`: `items` below `a` is
/// `a.items`, `[].id` below `a` is `a[].id`, and the root below `a` is `a`.
pub fn under(prefix: &str, path: &str) -> String {
    match (prefix.is_empty(), path.is_empty(), path.starts_with('[')) {
        (true, _, _) => path.to_string(),
        (false, true, _) => prefix.to_string(),
        (false, false, true) => format!("{prefix}{path}"),
        (false, false, false) => format!("{prefix}.{path}"),
    }
}

/// Where a shape was read from. A provider's sources, in the order the first that applies
/// gives the route's shape: a document, then (spec #23) a zod schema, then the object
/// literals the handler returns. A consumer's: the field reads on the parsed response.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// A provider document, by its repository-relative path.
    Document(String),
    /// The zod schema a route handler's response passes through or is typed with (spec #23), by
    /// the repository-relative path of the file holding the handler.
    Zod(String),
    /// The object literals a route handler returns (spec #23), by the repository-relative path
    /// of the file holding the handler.
    Literals(String),
    /// The field reads of a consumer on the parsed response (spec #23), in the file holding
    /// the consumer's call site.
    Reads(String),
}

impl Source {
    /// Where the shape comes from, as people see it: the file and what read it, such as
    /// `openapi.yaml (document)` or `src/orders.ts (zod)`.
    pub fn at(&self) -> String {
        match self {
            Source::Document(path) => format!("{path} (document)"),
            Source::Zod(path) => format!("{path} (zod)"),
            Source::Literals(path) => format!("{path} (literals)"),
            Source::Reads(path) => format!("{path} (reads)"),
        }
    }

    /// Whether a provider's shape is declared (a document, a schema) rather than read from what
    /// code does (object literals): drift is `exact` only against a declared one, on a consumer
    /// path a declared type gives (`Shape::declared`).
    fn declared(&self) -> bool {
        match self {
            Source::Document(_) | Source::Zod(_) => true,
            Source::Literals(_) | Source::Reads(_) => false,
        }
    }
}

/// `path` as shown to people: the empty path is `(root)`.
pub fn shown(path: &str) -> &str {
    if path.is_empty() { "(root)" } else { path }
}

/// The field paths a consumer reads that the provider does not return (spec #23).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Drift {
    /// The consumer's paths the provider's shape does not contain, each the shallowest one
    /// of its branch (nothing below a missing path is listed), with the level of that report:
    /// `exact` when the provider's shape is declared and the path comes from a declared type,
    /// else `inferred`. It is not the link's level.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub missing: BTreeMap<String, Level>,
    /// The consumer's paths below an open part of the provider's shape, each with that open
    /// path: not checked, and never drift.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub unchecked: BTreeMap<String, String>,
}

/// The drift of `consumer` against `provider`, compared from the root; `None` when every path
/// the consumer reads is checked and present. A path is checked one step at a time: the first
/// step below an open part is not checked, the first step the provider lacks is missing, and
/// nothing is checked past a step the provider knows to be a scalar (`name.length`), nor past
/// `length` below a field the provider returns as an array, which is no field of it.
pub fn drift(provider: &Shape, consumer: &Shape) -> Option<Drift> {
    if provider.scalars.contains("") {
        return None;
    }
    let present = |q: &str| provider.fields.contains(q) || provider.open.contains(q);
    let below = |q: &str, open: &str| {
        open.is_empty()
            || q.strip_prefix(open)
                .is_some_and(|r| r.starts_with(['.', '[']))
    };
    let guessed = |q: &str| provider.guessed.iter().any(|g| q == g || below(q, g));
    let level = |q: &str| {
        let declared = provider.source.declared() && consumer.declared.contains(q);
        match declared && !guessed(q) {
            true => Level::Exact,
            false => Level::Inferred,
        }
    };
    let mut missing = BTreeMap::new();
    let mut unchecked = BTreeMap::new();
    for path in &consumer.fields {
        for q in prefixes(path) {
            if let Some(open) = provider.open.iter().find(|o| below(q, o)) {
                unchecked.insert(q.to_string(), open.clone());
                break;
            }
            let parent = match q {
                "length" => Some(""),
                q => q.strip_suffix(".length"),
            };
            if parent.is_some_and(|p| present(&format!("{p}[]"))) {
                break;
            }
            if !present(q) {
                // A field the consumer's type marks optional may be absent: neither it nor
                // what lies below it is drift.
                if consumer.optional.contains(q) {
                    break;
                }
                missing.insert(q.to_string(), level(q));
                break;
            }
            if provider.scalars.contains(q) {
                break;
            }
        }
    }
    (!missing.is_empty() || !unchecked.is_empty()).then_some(Drift { missing, unchecked })
}

/// Each path from the root down to `path`, shallowest first: `items`, `items[]`, `items[].id`.
pub(crate) fn prefixes(path: &str) -> impl Iterator<Item = &str> {
    let ends = (path.char_indices())
        .filter(|&(i, c)| i > 0 && (c == '.' || c == '['))
        .map(|(i, _)| i);
    ends.chain([path.len()]).map(|end| &path[..end])
}

#[cfg(test)]
mod tests {
    use super::{GRAFT_BUDGET, Import, PACKAGE_HOPS, Shape, Source, Want, drift, fill};
    use crate::contract::Level;

    fn set(paths: &[&str]) -> std::collections::BTreeSet<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn document(fields: &[&str], open: &[&str], scalars: &[&str]) -> Shape {
        Shape {
            source: Source::Document("openapi.yaml".into()),
            fields: set(fields),
            open: set(open),
            scalars: set(scalars),
            declared: set(&[]),
            optional: set(&[]),
            imports: Default::default(),
            guessed: Default::default(),
        }
    }

    fn reads(paths: &[&str]) -> Shape {
        Shape {
            source: Source::Reads("src/a.ts".into()),
            fields: set(paths),
            open: set(&[]),
            scalars: set(&[]),
            declared: set(&[]),
            optional: set(&[]),
            imports: Default::default(),
            guessed: Default::default(),
        }
    }

    fn import(at: &str, want: Want) -> Import {
        Import::new(at, "src/a.ts", ("@acme/shared", "S".into()), want)
    }

    #[test]
    fn an_import_takes_the_view_its_want_reads() {
        let mut stored = document(&["id", "tags", "tags[]"], &["tags[]"], &["id"]);
        stored.declared = set(&["id", "note"]);
        stored.optional = set(&["note"]);
        let mut provider = document(&["extra"], &[], &[]);
        provider.imports.insert(import("item", Want::Zod));
        let filled = fill(&provider, (), &mut |_, _| Some(((), &stored, Level::Exact)));
        assert!(filled.imports.is_empty());
        assert_eq!(
            filled.fields,
            set(&["extra", "item", "item.id", "item.tags", "item.tags[]"])
        );
        assert_eq!(filled.open, set(&["item.tags[]"]));
        assert_eq!(filled.scalars, set(&["item.id"]));
        // A consumer takes the declared paths; one it reads is certain.
        let mut consumer = reads(&["note"]);
        consumer.imports.insert(import("", Want::Type));
        let filled = fill(&consumer, (), &mut |_, _| Some(((), &stored, Level::Exact)));
        assert_eq!(filled.fields, set(&["id", "note"]));
        assert_eq!(filled.declared, set(&["id", "note"]));
        assert!(filled.optional.is_empty());
        // Unresolved: open on a provider, nothing on a consumer.
        let none = fill(&provider, (), &mut |_, _| None);
        assert_eq!(none.open, set(&["item"]));
        assert_eq!(fill(&consumer, (), &mut |_, _| None).fields, set(&["note"]));
    }

    #[test]
    fn paths_grafted_through_a_resolution_not_exact_never_make_drift_exact() {
        let mut stored = document(&["id"], &[], &[]);
        stored.declared = set(&["id", "total"]);
        let mut consumer = reads(&["name"]);
        consumer.declared = set(&["name"]);
        consumer.imports.insert(import("", Want::Type));
        let mut provider = Shape::new(Source::Zod("src/p.ts".into()));
        provider.imports.insert(import("", Want::Zod));
        let local = document(&["id"], &[], &[]);
        for (level, expect) in [
            (Level::Exact, Level::Exact),
            (Level::Inferred, Level::Inferred),
        ] {
            // The provider's schema came through the package, the consumer's paths are local.
            let filled = fill(&provider, (), &mut |_, _| Some(((), &stored, level)));
            let found = drift(&filled, &consumer).unwrap();
            assert_eq!(found.missing["name"], expect, "provider through {level:?}");
            // The consumer's type came through the package, the provider is a document.
            let filled = fill(&consumer, (), &mut |_, _| Some(((), &stored, level)));
            let found = drift(&local, &filled).unwrap();
            assert_eq!(found.missing["total"], expect, "consumer through {level:?}");
        }
    }

    #[test]
    fn a_provider_typed_with_a_package_type_takes_its_open_parts_only() {
        let mut stored = document(&["labels", "title"], &["labels"], &[]);
        stored.declared = set(&["labels", "title"]);
        let mut provider = document(&["body"], &[], &[]);
        provider.imports.insert(import("body", Want::Opens));
        let filled = fill(&provider, (), &mut |_, _| Some(((), &stored, Level::Exact)));
        assert_eq!(filled.fields, set(&["body", "body.labels"]));
        assert_eq!(filled.open, set(&["body.labels"]));
        assert!(fill(&provider, (), &mut |_, _| None).open.is_empty());
    }

    #[test]
    fn imports_past_the_package_depth_limit_are_unresolved() {
        // Package `k` keeps `f` and imports the next one's shape at `next`.
        let mut kept = document(&["f"], &[], &[]);
        kept.imports.insert(import("next", Want::Literals));
        let mut side = Shape::new(Source::Literals("src/a.ts".into()));
        side.imports.insert(import("", Want::Literals));
        let mut looked = 0;
        let filled = fill(&side, 0, &mut |depth: usize, _| {
            looked += 1;
            Some((depth + 1, &kept, Level::Exact))
        });
        assert_eq!(looked, PACKAGE_HOPS);
        let at = |n: usize| vec!["next"; n].join(".");
        assert!(
            filled
                .fields
                .contains(&format!("{}.f", at(PACKAGE_HOPS - 1)))
        );
        assert!(!filled.fields.contains(&format!("{}.f", at(PACKAGE_HOPS))));
        assert_eq!(filled.open, set(&[&at(PACKAGE_HOPS)]));
    }

    #[test]
    fn imports_past_the_graft_budget_are_unresolved() {
        let kept = document(&["f"], &[], &[]);
        let mut side = Shape::new(Source::Literals("src/a.ts".into()));
        let paths: Vec<String> = (0..GRAFT_BUDGET).map(|i| format!("p{i:06}")).collect();
        for p in &paths {
            side.imports.insert(import(p, Want::Literals));
        }
        let filled = fill(&side, (), &mut |_, _| Some(((), &kept, Level::Exact)));
        let first = &paths[0];
        let last = &paths[paths.len() - 1];
        assert!(filled.fields.contains(&format!("{first}.f")));
        assert!(!filled.open.contains(first));
        assert!(!filled.fields.contains(&format!("{last}.f")));
        assert!(filled.open.contains(last));
    }

    #[test]
    fn field_paths_compare_from_the_root() {
        let orders = document(
            &[
                "[]",
                "[].id",
                "[].items",
                "[].items[]",
                "[].items[].sku",
                "[].product",
                "meta",
            ],
            &["[].product"],
            &["[].id", "[].items[].sku"],
        );
        // (consumer paths, missing, not checked as (path, open path))
        type Case<'c> = (&'c [&'c str], &'c [&'c str], &'c [(&'c str, &'c str)]);
        let cases: &[Case] = &[
            (&["[]", "[].id"], &[], &[]),
            (&["[]", "[].total"], &["[].total"], &[]),
            // The shallowest missing path is the drift; what lies below it is not listed.
            (
                &["[]", "[].customer", "[].customer.name"],
                &["[].customer"],
                &[],
            ),
            (
                &["[]", "[].items", "[].items[]", "[].items[].qty"],
                &["[].items[].qty"],
                &[],
            ),
            // An open part is present itself; a read below it is not checked.
            (&["[]", "[].product"], &[], &[]),
            (
                &[
                    "[]",
                    "[].product",
                    "[].product.name",
                    "[].product.name.first",
                ],
                &[],
                &[("[].product.name", "[].product")],
            ),
            // `meta` is not an array: an element of it is missing.
            (&["meta", "meta[]"], &["meta[]"], &[]),
            // The prefixes a consumer shape lists need not be there.
            (&["[].items[].sku"], &[], &[]),
            (&["count"], &["count"], &[]),
            (&["[].total", "count"], &["[].total", "count"], &[]),
            // A read below a scalar leaf is checked only up to the leaf.
            (&["[]", "[].id", "[].id.length"], &[], &[]),
            (&["[].items[].sku.x.y"], &[], &[]),
            // `length` of an array the provider returns is no field of it.
            (&["[]", "[].items", "[].items.length"], &[], &[]),
            (&["meta", "meta.length"], &["meta.length"], &[]),
            (&["length"], &[], &[]),
        ];
        for &(consumer, missing, unchecked) in cases {
            let found = drift(&orders, &reads(consumer));
            let got: Vec<&str> = (found.iter())
                .flat_map(|d| d.missing.keys().map(String::as_str))
                .collect();
            assert_eq!(got, missing, "missing for {consumer:?}");
            let got: Vec<(&str, &str)> = (found.iter())
                .flat_map(|d| d.unchecked.iter().map(|(p, o)| (p.as_str(), o.as_str())))
                .collect();
            assert_eq!(got, unchecked, "not checked for {consumer:?}");
            let clean = missing.is_empty() && unchecked.is_empty();
            assert_eq!(found.is_none(), clean, "drift for {consumer:?}");
        }
    }

    #[test]
    fn a_path_one_side_of_a_merge_requires_is_not_optional() {
        let provider = document(&["id"], &[], &[]);
        let mut declares = reads(&["id", "cursor"]);
        declares.optional = set(&["cursor"]);
        let reads_it = reads(&["cursor"]);
        let mut forward = declares.clone();
        forward.merge(&reads_it);
        let mut backward = reads_it.clone();
        backward.merge(&declares);
        for merged in [forward, backward] {
            assert!(merged.optional.is_empty());
            let found = drift(&provider, &merged).unwrap();
            assert_eq!(found.missing.keys().collect::<Vec<_>>(), ["cursor"]);
        }
        // A side that does not mention the path leaves it optional.
        let mut merged = declares.clone();
        merged.merge(&reads(&["id"]));
        assert_eq!(merged.optional, set(&["cursor"]));
    }

    #[test]
    fn an_optional_path_the_provider_lacks_is_no_drift() {
        let provider = document(&["id", "owner"], &[], &[]);
        let mut consumer = reads(&["id", "nick", "nick.first", "owner", "owner.zip", "req"]);
        consumer.optional = set(&["nick", "owner.zip"]);
        let found = drift(&provider, &consumer).unwrap();
        let missing: Vec<&str> = found.missing.keys().map(String::as_str).collect();
        assert_eq!(missing, ["req"]);
        // A path the provider has is checked as ever.
        let mut consumer = reads(&["id"]);
        consumer.optional = set(&["id"]);
        assert_eq!(drift(&provider, &consumer), None);
    }

    #[test]
    fn an_open_root_checks_nothing() {
        let open = document(&[], &[""], &[]);
        let found = drift(&open, &reads(&["id", "items", "items[]"])).unwrap();
        assert!(found.missing.is_empty());
        let unchecked: Vec<(&str, &str)> = (found.unchecked.iter())
            .map(|(p, o)| (p.as_str(), o.as_str()))
            .collect();
        assert_eq!(unchecked, [("id", ""), ("items", "")]);
    }

    #[test]
    fn a_scalar_root_has_no_fields_to_miss() {
        let scalar = document(&[], &[], &[""]);
        assert_eq!(drift(&scalar, &reads(&["length", "x", "x.y"])), None);
    }

    #[test]
    fn each_missing_path_has_its_own_level() {
        let provider = document(&["id"], &[], &[]);
        let mut consumer = reads(&["name", "total"]);
        consumer.declared.insert("total".into());
        let found = drift(&provider, &consumer).unwrap();
        let levels: Vec<(&str, Level)> = (found.missing.iter())
            .map(|(p, l)| (p.as_str(), *l))
            .collect();
        // A field read is inferred; a declared consumer path against a document is exact.
        assert_eq!(levels, [("name", Level::Inferred), ("total", Level::Exact)]);
        let mut read = document(&["id"], &[], &[]);
        read.source = Source::Reads("src/b.ts".into());
        let found = drift(&read, &consumer).unwrap();
        assert_eq!(found.missing["total"], Level::Inferred);
        // A zod schema is declared like a document; the object literals of a handler are not.
        let mut zod = document(&["id"], &[], &[]);
        zod.source = Source::Zod("src/c.ts".into());
        assert_eq!(
            drift(&zod, &consumer).unwrap().missing["total"],
            Level::Exact
        );
        zod.source = Source::Literals("src/c.ts".into());
        assert_eq!(
            drift(&zod, &consumer).unwrap().missing["total"],
            Level::Inferred
        );
    }
}
