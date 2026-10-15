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
}

/// Where a shape was read from. A provider's sources, in the order the first that applies
/// gives the route's shape: a document, then (spec #23, later) a zod schema, then the object
/// literals the handler returns. A consumer's: the field reads on the parsed response.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// A provider document, by its repository-relative path.
    Document(String),
    /// The field reads of a consumer on the parsed response (spec #23), in the file holding
    /// the consumer's call site.
    Reads(String),
}

impl Source {
    /// The document, or the place in code, the shape comes from.
    pub fn at(&self) -> &str {
        match self {
            Source::Document(path) | Source::Reads(path) => path,
        }
    }

    /// Whether the shape is declared (a document, a schema, a type) rather than read from
    /// what code does (field reads, object literals): drift between two declared shapes is
    /// `exact`, any other `inferred`.
    fn declared(&self) -> bool {
        match self {
            Source::Document(_) => true,
            Source::Reads(_) => false,
        }
    }
}

/// `path` as shown to people: the empty path is `(root)`.
pub fn shown(path: &str) -> &str {
    if path.is_empty() { "(root)" } else { path }
}

/// The field paths a consumer reads that the provider does not return (spec #23). It is the
/// level of the report, not of the link.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Drift {
    pub level: Level,
    /// The consumer's paths the provider's shape does not contain, each the shallowest one
    /// of its branch: nothing below a missing path is listed.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub missing: BTreeSet<String>,
    /// The consumer's paths below an open part of the provider's shape, each with that open
    /// path: not checked, and never drift.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub unchecked: BTreeMap<String, String>,
}

/// The drift of `consumer` against `provider`, compared from the root; `None` when every path
/// the consumer reads is checked and present. A path is checked one step at a time: the first
/// step below an open part is not checked, the first step the provider lacks is missing.
/// `length` below a field the provider returns as an array is no field of it.
pub fn drift(provider: &Shape, consumer: &Shape) -> Option<Drift> {
    let present = |q: &str| provider.fields.contains(q) || provider.open.contains(q);
    let below = |q: &str, open: &str| {
        open.is_empty()
            || q.strip_prefix(open)
                .is_some_and(|r| r.starts_with(['.', '[']))
    };
    let mut missing = BTreeSet::new();
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
                missing.insert(q.to_string());
                break;
            }
        }
    }
    let level = match provider.source.declared() && consumer.source.declared() {
        true => Level::Exact,
        false => Level::Inferred,
    };
    (!missing.is_empty() || !unchecked.is_empty()).then_some(Drift {
        level,
        missing,
        unchecked,
    })
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

    fn shape(source: Source, fields: &[&str], open: &[&str]) -> Shape {
        Shape {
            source,
            fields: fields.iter().map(|f| f.to_string()).collect(),
            open: open.iter().map(|o| o.to_string()).collect(),
        }
    }

    fn reads(paths: &[&str]) -> Shape {
        shape(Source::Reads("src/a.ts".into()), paths, &[])
    }

    #[test]
    fn field_paths_compare_from_the_root() {
        let orders = shape(
            Source::Document("openapi.yaml".into()),
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
            // `length` of an array the provider returns is no field of it.
            (&["[]", "[].items", "[].items.length"], &[], &[]),
            (&["meta", "meta.length"], &["meta.length"], &[]),
            (&["length"], &[], &[]),
        ];
        for &(consumer, missing, unchecked) in cases {
            let found = drift(&orders, &reads(consumer));
            let got: Vec<&str> = (found.iter())
                .flat_map(|d| d.missing.iter().map(String::as_str))
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
        let open = shape(Source::Document("openapi.yaml".into()), &[], &[""]);
        let found = drift(&open, &reads(&["id", "items", "items[]"])).unwrap();
        assert!(found.missing.is_empty());
        let unchecked: Vec<(&str, &str)> = (found.unchecked.iter())
            .map(|(p, o)| (p.as_str(), o.as_str()))
            .collect();
        assert_eq!(unchecked, [("id", ""), ("items", "")]);
    }

    #[test]
    fn drift_from_field_reads_is_inferred() {
        let document = shape(Source::Document("openapi.yaml".into()), &["id"], &[]);
        let found = drift(&document, &reads(&["name"])).unwrap();
        assert_eq!(found.level, Level::Inferred);
    }
}
