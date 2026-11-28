use crate::common::fixture_repo;
use meada::engine::Engine;
use serde_json::Value;
use std::sync::OnceLock;

fn call(caller: &str, callee: &str) -> &'static Value {
    static EXPORT: OnceLock<Value> = OnceLock::new();
    let export = EXPORT.get_or_init(|| {
        let repo = fixture_repo(
            "ts_library_receivers",
            "ts-library-receivers",
            Some("https://example.com/acme/receivers.git"),
        );
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap()
    });
    let caller = format!("::{caller}");
    let callee = format!(".{callee}#0");
    let calls: Vec<_> = export["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c["kind"].is_null()
                && c["caller"].as_str().is_some_and(|id| id.ends_with(&caller))
                && c["id"].as_str().is_some_and(|id| id.ends_with(&callee))
        })
        .collect();
    assert_eq!(calls.len(), 1, "{caller}, {callee}: {calls:?}");
    calls[0]
}

fn no_candidate(row: &Value) {
    assert!(row["level"].is_null() && row["target"].is_null(), "{row}");
    assert!(
        row["candidates"].as_array().is_none_or(Vec::is_empty),
        "{row}"
    );
}

#[test]
fn global_values_have_no_repository_candidate() {
    for name in ["get", "json", "info", "parse", "max", "keys"] {
        no_candidate(call("globals", name));
    }
}

#[test]
fn a_global_annotation_has_no_production_candidate() {
    no_candidate(call("annotated", "json"));
}

#[test]
fn an_outside_annotation_keeps_test_literal_and_naming_candidates() {
    let row = call("outside", "info");
    assert_eq!(row["level"], "ambiguous", "{row}");
    let mut targets: Vec<_> = row["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().rsplit("::").next().unwrap())
        .collect();
    targets.sort_unstable();
    assert_eq!(targets, ["Audit.info", "Fake.info", "q.info"], "{row}");
}

fn candidates(row: &Value) -> Vec<&str> {
    row["target"]
        .as_str()
        .into_iter()
        .chain(
            row["candidates"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .map(|id| id.rsplit("::").next().unwrap())
        .collect()
}

#[test]
fn a_repository_package_type_keeps_todays_answer() {
    for source in ["shared", "subpath"] {
        let row = call(source, "log");
        assert_eq!(row["level"], "inferred", "{row}");
        assert_eq!(candidates(row), ["Logger.log"], "{row}");
    }
}

#[test]
fn shadowed_globals_keep_todays_answer() {
    let map = call("shadowMap", "get");
    assert_eq!(map["level"], "exact", "{map}");
    assert_eq!(candidates(map), ["Map.get"]);
    let log = call("shadowConsole", "info");
    assert_eq!(log["level"], "ambiguous", "{log}");
    assert_eq!(candidates(log), ["Fake.info", "Audit.info", "Store.info"]);
}

#[test]
fn an_inline_chain_keeps_todays_answer() {
    let row = call("inline", "get");
    assert_eq!(row["level"], "ambiguous", "{row}");
    assert_eq!(candidates(row), ["Store.get", "Map.get"]);
}

#[test]
fn a_tsconfig_alias_type_keeps_todays_answer() {
    for source in ["alias", "base"] {
        let row = call(source, "load");
        assert_eq!(row["level"], "exact", "{row}");
        assert_eq!(candidates(row), ["Repo.load"]);
    }
}

#[test]
fn a_package_imports_specifier_keeps_todays_answer() {
    let row = call("internal", "load");
    assert_eq!(row["level"], "ambiguous", "{row}");
    assert_eq!(candidates(row), ["Cache.load", "Repo.load"]);
}

#[test]
fn only_the_global_call_in_a_chain_loses_its_candidates() {
    no_candidate(call("parsed", "parse"));
    assert_eq!(candidates(call("parsed", "get")), ["Store.get", "Map.get"]);
}

#[test]
fn a_generic_global_and_a_shadowed_fetch_use_their_own_binding() {
    no_candidate(call("generic", "get"));
    assert_eq!(candidates(call("localFetch", "json")), ["Store.json"]);
    assert_eq!(candidates(call("awaited", "get")), ["Store.get", "Map.get"]);
}

#[test]
fn an_instance_annotation_belongs_to_the_instance_read() {
    assert_eq!(
        candidates(call("Service.instanceLog", "info")),
        ["Fake.info", "Audit.info", "q.info"]
    );
    assert_eq!(
        candidates(call("Service.staticLog", "info")),
        ["Fake.info", "Audit.info", "Store.info"]
    );
}

#[test]
fn an_imported_promise_type_cannot_filter_its_awaited_value() {
    let row = call("fromImportedPromise", "get");
    assert_eq!(row["level"], "ambiguous", "{row}");
    assert_eq!(candidates(row), ["Store.get", "Map.get"], "{row}");
}

#[test]
fn only_an_unwrapped_result_types_an_awaited_value() {
    let row = call("fromPeer", "listen");
    assert_eq!(row["level"], "ambiguous", "{row}");
    assert_eq!(candidates(row), ["Client.listen", "Peer.listen"], "{row}");
    no_candidate(call("fromResponse", "json"));
}

#[test]
fn an_outside_annotation_on_a_repository_literal_keeps_its_answer() {
    let row = call("literal", "check");
    assert_eq!(row["level"], "exact", "{row}");
    assert_eq!(candidates(row), ["v.check"], "{row}");
}

#[test]
fn a_nested_or_field_literal_keeps_the_ordinary_member_lookup() {
    for caller in ["nested", "Service.checkValue"] {
        let row = call(caller, "check");
        assert_eq!(row["level"], "inferred", "{row}");
        assert_eq!(candidates(row), ["Store.check"], "{row}");
    }
}
