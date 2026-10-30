//! The effective configuration of named-workspace entries (ADR 0021 contract 6, #344) against
//! `tests/fixtures/effective-configuration.json`, the table the front end's WebAssembly reads
//! too. The function is pure: the same entries and facts give the same view on any machine, and
//! two contexts sharing a repository share no block.

use std::collections::BTreeMap;

use meada_core::effective::{Facts, effective};
use serde_json::{Value, json};

fn table() -> Value {
    let text = include_str!("../../tests/fixtures/effective-configuration.json");
    serde_json::from_str(text).unwrap()
}

/// `value` with every short identity of the table (`Y`, `A`, `B`) replaced by its full one.
fn spelled(value: &Value, table: &Value) -> Value {
    match value {
        Value::String(s) => table["identities"][s.as_str()]
            .clone()
            .as_str()
            .map_or_else(
                || Value::String(s.clone()),
                |identity| Value::String(identity.into()),
            ),
        Value::Array(items) => Value::Array(items.iter().map(|v| spelled(v, table)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let key = table["identities"][k.as_str()].as_str().unwrap_or(k);
                    (key.to_string(), spelled(v, table))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn facts_of(row: &Value, table: &Value) -> BTreeMap<String, Facts> {
    let facts = spelled(&row["facts"], table);
    serde_json::from_value::<BTreeMap<String, Value>>(facts)
        .unwrap()
        .into_iter()
        .map(|(identity, facts)| (identity, serde_json::from_value(facts).unwrap()))
        .collect()
}

fn entries_of(entries: &Value, table: &Value) -> Vec<Value> {
    // Only `repo`, a peer's `repo` and the identities are spelled out: blocks keep their text.
    entries
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let mut entry = entry.clone();
            let long = |short: &Value| table["identities"][short.as_str().unwrap()].clone();
            entry["repo"] = long(&entry["repo"]);
            if let Some(peers) = entry.get_mut("peer").and_then(Value::as_array_mut) {
                for peer in peers {
                    peer["repo"] = long(&peer["repo"]);
                }
            }
            entry
        })
        .collect()
}

#[test]
fn every_row_of_the_table_is_decided_as_the_adr_says() {
    let table = table();
    for row in table["rows"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let entries = entries_of(&row["entries"], &table);
        let found = effective(&entries, &facts_of(row, &table));
        let expected = row["expect"].as_array().unwrap();
        assert_eq!(found.len(), expected.len(), "{name}");
        for (got, want) in found.iter().zip(expected) {
            let got = serde_json::to_value(got).unwrap();
            let codes: Vec<&str> = (got["diagnostics"].as_array().unwrap().iter())
                .map(|d| d["code"].as_str().unwrap())
                .collect();
            let repo = table["identities"][want["repo"].as_str().unwrap()].clone();
            assert_eq!(got["repo"], repo, "{name}");
            assert_eq!(got["source"], want["source"], "{name}");
            assert_eq!(got["coverage"], want["coverage"], "{name}");
            assert_eq!(got["ignored"], want["ignored"], "{name}");
            assert_eq!(json!(codes), want["diagnostics"], "{name}");
        }
    }
}

#[test]
fn two_contexts_sharing_a_repository_read_only_their_own_blocks() {
    let table = table();
    for row in table["contexts"].as_array().unwrap() {
        let facts = facts_of(row, &table);
        let first = effective(&entries_of(&row["first"], &table), &facts);
        let second = effective(&entries_of(&row["second"], &table), &facts);
        let y = |view: &[meada_core::effective::Effective]| {
            serde_json::to_value(&view[0].blocks).unwrap()
        };
        assert_eq!(y(&first), spelled(&row["blocks"]["first"], &table));
        assert_eq!(y(&second), spelled(&row["blocks"]["second"], &table));
        assert_ne!(y(&first), y(&second));
        // Deterministic: asking again gives the same view.
        assert_eq!(first, effective(&entries_of(&row["first"], &table), &facts));
    }
}
