//! `search` with kind `duplicate` lists the symbols whose text is the same in two or more
//! places of the workspace (#254); the export carries each symbol's hash.

use meada::engine::Engine;

use crate::common::{assert_golden, fixture_repo};

/// Repository `a` holds `slug` twice and a near copy of it (whitespace differs); its peer `b`
/// holds it once more. Both also hold `other`, a shorter function.
fn duplicates(case: &str) -> Engine {
    fixture_repo(
        case,
        "duplicates-b",
        Some("https://example.com/duplicates/b.git"),
    );
    let a = fixture_repo(
        case,
        "duplicates-a",
        Some("https://example.com/duplicates/a.git"),
    );
    Engine::open(&a).unwrap()
}

#[test]
fn three_exact_copies_are_one_group_and_the_near_copy_is_left_out() {
    let engine = duplicates("duplicates_search");

    let answer = engine.search("slug", Some("duplicate"), None).unwrap();

    assert_golden("duplicates.search-duplicate.txt", &answer);
}

#[test]
fn groups_go_by_span_length_longest_first() {
    let engine = duplicates("duplicates_order");

    let answer = engine.search("src", Some("duplicate"), None).unwrap();

    assert_golden("duplicates.search-duplicate-order.txt", &answer);
}

#[test]
fn the_query_matches_a_member_by_path_and_a_miss_says_so() {
    let engine = duplicates("duplicates_path");

    let by_path = engine.search("legacy", Some("duplicate"), None).unwrap();
    let none = engine.search("nothing", Some("duplicate"), None).unwrap();

    assert!(by_path.starts_with("3 copies\n"), "{by_path}");
    assert_eq!(none, "no matches for `nothing`\n");
}

#[test]
fn the_export_carries_the_hash_of_each_symbol() {
    let engine = duplicates("duplicates_export");

    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();

    let hashes: Vec<_> = export["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["hash"].as_str().unwrap().to_string())
        .collect();
    assert!(!hashes.is_empty());
    assert!(hashes.iter().all(|h| h.len() == 64));
}
