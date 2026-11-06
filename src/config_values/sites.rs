//! Exact locations named by declarations, resolved solely against the config index (#641).
//! Selectors compare complete scalar values, never redacted or cut values. Parsing and
//! matching are iterative; a query cannot expand beyond the keys already held by its file.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::Result;
use crate::document::events::Step;
use crate::index::Export;

/// One location, with the declaration spelling retained for selector-based queries.
#[derive(Clone, Debug)]
pub(crate) struct Site {
    pub id: String,
    pub via: String,
    pub path: String,
    pub key: String,
    pub line: usize,
}

impl Site {
    pub(crate) fn place(&self) -> String {
        format!("{}#{} (line {})", self.path, self.key, self.line)
    }
}

/// The declared keys named by canonical identity, local locator or declaration selector.
pub(crate) fn found<'a>(exports: &'a [Export], text: &str) -> Vec<(usize, &'a Site)> {
    let mut seen = BTreeSet::new();
    exports
        .iter()
        .enumerate()
        .flat_map(|(n, e)| {
            let text = text
                .strip_prefix(&format!("{}::", e.repository))
                .unwrap_or(text);
            e.config_sites.iter().filter_map(move |s| {
                (s.via == text || format!("{}#{}", s.path, s.key) == text).then_some((n, s))
            })
        })
        .filter(|(_, s)| seen.insert(s.id.as_str()))
        .collect()
}

#[derive(Debug)]
enum Part {
    Step(Step),
    Select(Vec<Step>, String),
}

/// Position of `wanted` outside strings and nested brackets.
fn boundary(text: &str, wanted: u8) -> Option<usize> {
    let (mut quoted, mut escaped, mut depth) = (false, false, 0usize);
    for (n, byte) in text.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if byte == wanted && depth == 0 {
            return Some(n);
        }
        match byte {
            b'"' => quoted = true,
            b'[' => depth += 1,
            b']' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

fn parts(mut text: &str, selectors: bool) -> Option<Vec<Part>> {
    if text == "." {
        return Some(Vec::new());
    }
    let mut parts = Vec::new();
    while !text.is_empty() {
        if let Some(rest) = text.strip_prefix('[') {
            let end = boundary(rest, b']')?;
            let inner = &rest[..end];
            let part = if let Some(at) = boundary(inner, b'=') {
                if !selectors {
                    return None;
                }
                let keys = parts_without_selectors(inner[..at].trim())?;
                if keys.is_empty() {
                    return None;
                }
                let literal = inner[at + 1..].trim();
                let value = if literal.starts_with('"') {
                    serde_json::from_str::<String>(literal).ok()?
                } else {
                    if literal.is_empty() || literal.contains(['[', ']']) {
                        return None;
                    }
                    literal.to_string()
                };
                Part::Select(keys, value)
            } else if inner.starts_with('"') {
                Part::Step(Step::Key(serde_json::from_str(inner).ok()?))
            } else {
                if inner.is_empty() || !inner.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                Part::Step(Step::Index(inner.parse().ok()?))
            };
            parts.push(part);
            text = &rest[end + 1..];
        } else {
            let end = text.find(['.', '[']).unwrap_or(text.len());
            let key = &text[..end];
            if !super::plain(key) {
                return None;
            }
            parts.push(Part::Step(Step::Key(key.to_string())));
            text = &text[end..];
        }
        if let Some(rest) = text.strip_prefix('.') {
            if rest.is_empty() || rest.starts_with('[') {
                return None;
            }
            text = rest;
        } else if !text.is_empty() && !text.starts_with('[') {
            return None;
        }
    }
    (!parts.is_empty()).then_some(parts)
}

fn parts_without_selectors(text: &str) -> Option<Vec<Step>> {
    parts(text, false)?
        .into_iter()
        .map(|p| match p {
            Part::Step(step) => Some(step),
            Part::Select(..) => None,
        })
        .collect()
}

struct Scalar {
    path: Vec<Step>,
    line: usize,
}

/// Resolve one `<file>#<key path>` against indexed scalars and their parent keys. No source
/// path is followed; ignored, unreadable, missing and out-of-repository files have no rows.
pub(crate) fn locate(db: &Connection, identity: &str, via: &str) -> Result<Vec<Site>> {
    let Some((path, key)) = via.split_once('#') else {
        return Ok(Vec::new());
    };
    let Some(wanted) = parts(key, true) else {
        return Ok(Vec::new());
    };
    let mut query = db.prepare(
        "SELECT key, value, cut, line, type FROM config_values WHERE path = ?1 ORDER BY key, line",
    )?;
    let rows = query.query_map([path], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, bool>(2)?,
            r.get::<_, i64>(3)? as usize,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut scalars = Vec::new();
    let mut values: BTreeMap<String, Vec<(Option<String>, bool)>> = BTreeMap::new();
    for row in rows {
        let (key, value, cut, line, kind) = row?;
        let Some(path) = parts_without_selectors(&key) else {
            continue;
        };
        let literal = matches!(kind.as_str(), "string" | "number" | "boolean");
        values
            .entry(key)
            .or_default()
            .push((value, !cut && literal));
        scalars.push(Scalar { path, line });
    }
    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    for scalar in scalars {
        if scalar.path.len() < wanted.len() {
            continue;
        }
        let matches = wanted
            .iter()
            .zip(&scalar.path)
            .enumerate()
            .all(|(at, (part, step))| match part {
                Part::Step(wanted) => wanted == step,
                Part::Select(keys, value) if matches!(step, Step::Index(_)) => {
                    let mut selected = scalar.path[..=at].to_vec();
                    selected.extend(keys.iter().cloned());
                    let selected = super::key_path(&selected);
                    values.get(&selected).is_some_and(|rows| {
                        rows.iter()
                            .all(|(v, literal)| *literal && v.as_deref() == Some(value.as_str()))
                    })
                }
                Part::Select(..) => false,
            });
        if matches {
            let key = super::key_path(&scalar.path[..wanted.len()]);
            found
                .entry(key)
                .and_modify(|line| *line = (*line).min(scalar.line))
                .or_insert(scalar.line);
        }
    }
    Ok(found
        .into_iter()
        .map(|(key, line)| Site {
            id: format!("{identity}::{path}#{key}"),
            via: via.to_string(),
            path: path.to_string(),
            key,
            line,
        })
        .collect())
}
