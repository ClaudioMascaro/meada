//! Shapes (spec #21, ADR 0009): what a contract carries in its payload, as a set of field paths,
//! in one form whatever it was read from, so shape drift (spec #23) compares shapes without
//! caring about their source.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

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

/// Where a shape was read from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// A provider document, by its repository-relative path.
    Document(String),
}

impl Source {
    /// The document, or the place in code, the shape comes from.
    pub fn at(&self) -> &str {
        match self {
            Source::Document(path) => path,
        }
    }
}

/// `path` as shown to people: the empty path is `(root)`.
pub fn shown(path: &str) -> &str {
    if path.is_empty() { "(root)" } else { path }
}
