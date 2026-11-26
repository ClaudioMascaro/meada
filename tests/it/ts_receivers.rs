//! TypeScript calls by the receiver's class and the by-name answer when dispatch is open.

use crate::common::fixture_repo;
use meada::engine::Engine;
use serde_json::Value;
use std::sync::OnceLock;

fn export() -> &'static Value {
    static EXPORT: OnceLock<Value> = OnceLock::new();
    EXPORT.get_or_init(|| {
        let repo = fixture_repo(
            "ts_receivers",
            "ts-receivers",
            Some("https://example.com/acme/receivers.git"),
        );
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap()
    })
}

// Each caller in the fixture makes exactly one call of this method.
fn call(plugin: &str, file: &str, caller: &str, method: &str) -> &'static Value {
    let caller = format!("example.com/acme/receivers::plugins/{plugin}/src/{file}.ts::{caller}");
    let calls: Vec<_> = export()["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            if row.get("kind").is_some() || row["caller"] != caller {
                return false;
            }
            let id = row["id"].as_str().unwrap().rsplit_once('#').unwrap().0;
            id.rsplit('.').next() == Some(method)
        })
        .collect();
    assert_eq!(calls.len(), 1, "{caller}, {method}: {calls:?}");
    calls[0]
}

fn relative(id: &Value) -> &str {
    id.as_str()
        .unwrap()
        .strip_prefix("example.com/acme/receivers::plugins/")
        .unwrap()
}

fn exact(row: &Value, target: &str) {
    assert_eq!(row["level"], "exact", "{row}");
    assert_eq!(relative(&row["target"]), target);
}

fn by_name(row: &Value, targets: &[&str]) {
    if targets.len() == 1 {
        assert_eq!(row["level"], "inferred", "{row}");
        assert_eq!(relative(&row["target"]), targets[0]);
    } else {
        assert_eq!(row["level"], "ambiguous", "{row}");
        let mut found: Vec<_> = row["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(relative)
            .collect();
        found.sort_unstable();
        assert_eq!(found, targets, "{row}");
    }
}

const SEND: &[&str] = &[
    "a/src/base.ts::Base.send",
    "a/src/base.ts::Sub.send",
    "a/src/client.ts::Client.send",
    "b/src/client.ts::Client.send",
    "b/src/remote.ts::Remote.send",
];

fn both_clients(caller: &str) {
    for plugin in ["a", "b"] {
        exact(
            call(plugin, "client", caller, "send"),
            &format!("{plugin}/src/client.ts::Client.send"),
        );
    }
}

#[test]
fn a_new_local_resolves_to_its_plugins_class() {
    both_clients("fresh");
}
#[test]
fn a_typed_const_resolves() {
    both_clients("annotated");
}
#[test]
fn a_typed_parameter_resolves() {
    both_clients("param");
}
#[test]
fn an_optional_parameter_is_its_class() {
    both_clients("optional");
}
#[test]
fn await_optional_chain_and_parentheses_resolve_like_the_plain_call() {
    for caller in ["awaited", "optional", "parens"] {
        both_clients(caller);
    }
}
#[test]
fn a_method_found_in_a_base_resolves_to_the_base() {
    exact(
        call("a", "base", "onlyBase", "listen"),
        "a/src/base.ts::Base.listen",
    );
}
#[test]
fn an_override_resolves_on_the_subclass() {
    exact(
        call("a", "base", "onSub", "send"),
        "a/src/base.ts::Sub.send",
    );
}
#[test]
fn a_base_receiver_with_an_heir_keeps_todays_answer() {
    by_name(call("a", "base", "onBase", "send"), SEND);
}
#[test]
fn an_heir_in_another_package_keeps_todays_answer() {
    by_name(
        call("a", "base", "onRemoteBase", "cross"),
        &["a/src/base.ts::Base.cross", "b/src/remote.ts::Remote.cross"],
    );
}
#[test]
fn a_class_implementing_the_receivers_class_keeps_todays_answer() {
    by_name(
        call("a", "cases", "structuralClass", "notify"),
        &[
            "a/src/cases.ts::Impl.notify",
            "a/src/cases.ts::Implemented.notify",
        ],
    );
}
#[test]
fn a_grandchild_override_keeps_todays_answer() {
    by_name(
        call("a", "cases", "grandchild", "relay"),
        &["a/src/cases.ts::Grand.relay", "a/src/cases.ts::Leaf.relay"],
    );
}
#[test]
fn this_and_super_calls_follow_the_heir_rule() {
    exact(
        call("a", "base", "Base.own", "send"),
        "a/src/base.ts::Base.send",
    );
    by_name(call("a", "base", "Base.wrapped", "send"), SEND);
    exact(
        call("a", "base", "Sub.inherited", "listen"),
        "a/src/base.ts::Base.listen",
    );
    exact(
        call("a", "base", "Sub.send", "send"),
        "a/src/base.ts::Base.send",
    );
    by_name(
        call("a", "cases", "Inherits.run", "hit"),
        &["a/src/cases.ts::Overrides.hit", "a/src/cases.ts::Root.hit"],
    );
}
#[test]
fn a_field_shadowing_a_base_method_keeps_todays_answer() {
    for caller in ["shadow", "Sub2.run"] {
        by_name(
            call("a", "cases", caller, "ping"),
            &["a/src/cases.ts::BaseField.ping"],
        );
    }
}
#[test]
fn uncertain_receivers_keep_todays_answer() {
    for caller in [
        "mutable",
        "structural",
        "union",
        "anyType",
        "outside",
        "wrappedImport",
        "partial",
        "abstractMethod",
        "overload",
    ] {
        by_name(call("a", "guards", caller, "send"), SEND);
    }
    by_name(
        call("a", "guards", "generic", "generic"),
        &[
            "a/src/guards.ts::Other.generic",
            "a/src/guards.ts::T.generic",
        ],
    );
}
#[test]
fn a_union_with_null_keeps_todays_answer() {
    by_name(call("a", "guards", "nullable", "send"), SEND);
}
#[test]
fn a_class_under_another_name_never_resolves_to_another() {
    for caller in ["byAlias", "byDefault", "byNamespace"] {
        let row = call("a", "aliases", caller, "send");
        if row["level"] == "exact" {
            exact(row, "a/src/client.ts::Client.send");
        } else {
            by_name(row, SEND);
        }
    }
}
#[test]
fn merged_and_ambient_declarations_keep_todays_answer() {
    for caller in ["merged", "ambient"] {
        by_name(call("a", "guards", caller, "send"), SEND);
    }
}

#[test]
fn a_receiver_belongs_to_the_block_that_declares_it() {
    for (method, target) in [
        ("send", "a/src/client.ts::Client.send"),
        ("listen", "b/src/client.ts::Client.listen"),
    ] {
        exact(call("a", "blocks", "separate", method), target);
    }
}

#[test]
fn a_block_local_does_not_leak_into_the_enclosing_function() {
    let row = call("a", "blocks", "outsideBlock", "send");
    if row["level"] == "exact" {
        exact(row, "b/src/client.ts::Client.send");
    } else {
        by_name(row, SEND);
    }
}

#[test]
fn a_local_inside_try_resolves_in_that_block() {
    exact(
        call("a", "blocks", "insideTry", "send"),
        "a/src/client.ts::Client.send",
    );
}

#[test]
fn switch_fallthrough_keeps_the_unbraced_local() {
    exact(
        call("a", "blocks", "fallthrough", "send"),
        "a/src/client.ts::Client.send",
    );
}

#[test]
fn switch_fallthrough_leaves_a_braced_local_behind() {
    exact(
        call("a", "blocks", "bracedFallthrough", "send"),
        "b/src/client.ts::Client.send",
    );
}

#[test]
fn a_call_before_the_local_declaration_has_no_typed_receiver() {
    by_name(call("a", "blocks", "beforeDeclaration", "send"), SEND);
}

fn field_target(caller: &str) {
    exact(
        call("a", "fields", caller, "send"),
        "a/src/client.ts::Client.send",
    );
}

#[test]
fn a_parameter_property_field_resolves() {
    field_target("S.go");
}

#[test]
fn an_annotated_field_resolves() {
    field_target("T.go");
}

#[test]
fn an_annotated_field_written_again_still_resolves() {
    field_target("T2.go");
}

#[test]
fn an_initialized_field_resolves() {
    field_target("V.go");
}

#[test]
fn a_field_assigned_once_in_the_constructor_resolves() {
    field_target("U.go");
}

#[test]
fn a_field_written_again_keeps_todays_answer() {
    for caller in ["Changes.go", "Parent.go", "Shared.go"] {
        by_name(call("a", "fields", caller, "send"), SEND);
    }
}

#[test]
fn a_write_in_an_unrelated_class_does_not_count() {
    field_target("Separate.go");
}

#[test]
fn a_bare_name_in_a_class_initializer_is_not_the_field() {
    exact(
        call("a", "fields", "V2", "send"),
        "b/src/client.ts::Client.send",
    );
}

#[test]
fn private_fields_count_only_writes_reachable_in_their_class() {
    for caller in ["PrivateStore.first", "PrivateStore.second"] {
        field_target(caller);
    }
}

#[test]
fn a_static_factory_result_resolves() {
    exact(
        call("a", "results", "fromStatic", "send"),
        "a/src/client.ts::Client.send",
    );
}
#[test]
fn an_awaited_async_result_resolves() {
    exact(
        call("a", "results", "fromAsync", "send"),
        "a/src/client.ts::Client.send",
    );
}
#[test]
fn an_unawaited_async_result_keeps_todays_answer() {
    by_name(call("a", "results", "withoutAwait", "send"), SEND);
}
#[test]
fn a_generic_result_keeps_todays_answer() {
    by_name(call("a", "results", "fromGeneric", "send"), SEND);
}
#[test]
fn a_result_typed_by_an_outside_class_keeps_todays_answer() {
    by_name(call("a", "results", "fromOutside", "send"), SEND);
}

#[test]
fn an_awaited_local_type_alias_does_not_unwrap_as_the_global_promise() {
    by_name(call("a", "shadowed-result", "shadowed", "send"), SEND);
}
