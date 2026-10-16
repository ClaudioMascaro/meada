//! Shapes (spec #21, ADR 0009): what a contract carries in its payload, as a set of field paths,
//! in one form whatever it was read from, so shape drift (spec #23) compares shapes without
//! caring about their source.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::contract::Level;

/// The field paths a payload carries. A path is dotted, with array elements written `[]`
/// (`items[].id`); a payload that is itself an array starts with `[]` (`[].id`). Every node
/// below the root is a field, so `items` and `items[]` are listed with `items[].id`. Presence
/// only: no type, format or requiredness is recorded.
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
        }
    }

    /// Adds the paths of `other`, read from the same side, to this shape.
    pub fn merge(&mut self, other: &Shape) {
        self.fields.extend(other.fields.iter().cloned());
        self.open.extend(other.open.iter().cloned());
        self.scalars.extend(other.scalars.iter().cloned());
        self.declared.extend(other.declared.iter().cloned());
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
    /// The document, or the place in code, the shape comes from.
    pub fn at(&self) -> &str {
        match self {
            Source::Document(path)
            | Source::Zod(path)
            | Source::Literals(path)
            | Source::Reads(path) => path,
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
    let level = |q: &str| match provider.source.declared() && consumer.declared.contains(q) {
        true => Level::Exact,
        false => Level::Inferred,
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
    use super::{Shape, Source, drift};
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
        }
    }

    fn reads(paths: &[&str]) -> Shape {
        Shape {
            source: Source::Reads("src/a.ts".into()),
            fields: set(paths),
            open: set(&[]),
            scalars: set(&[]),
            declared: set(&[]),
        }
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
