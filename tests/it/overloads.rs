//! Overloads of one method show as one set in `context` and `trace` (#397); the export keeps
//! every candidate.

use meada::engine::{Direction, Engine, Start};

use crate::common::{assert_golden, fixture_repo};

/// `context` and `trace` of `caller` in `fixture`, with the export's candidate count of its
/// first call, which the grouping must not change.
fn answers(fixture: &str, caller: &str) -> (String, String, usize) {
    let repo = fixture_repo(fixture, fixture, None);
    let engine = Engine::open(&repo).unwrap();
    let context = engine.context(caller, None).unwrap();
    let trace = engine
        .trace(Start::From(caller), Direction::Down, None)
        .unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let calls = export["calls"].as_array().unwrap();
    let ambiguous = calls.iter().find(|c| c["level"] == "ambiguous").unwrap();
    (
        context,
        trace,
        ambiguous["candidates"].as_array().unwrap().len(),
    )
}

#[test]
fn overloads_of_one_class_are_one_set() {
    let (context, trace, candidates) = answers("overloads-one", "ex.Box.run(Object)");
    assert_eq!(candidates, 3);
    assert_golden("overloads-one.context-run.txt", &context);
    assert_golden("overloads-one.trace-run.txt", &trace);
}

#[test]
fn overloads_in_two_classes_are_two_sets() {
    let (context, trace, candidates) = answers("overloads-two", "ex.Pair.run(Object)");
    assert_eq!(candidates, 4);
    assert_golden("overloads-two.context-run.txt", &context);
    assert_golden("overloads-two.trace-run.txt", &trace);
}

#[test]
fn a_method_beside_a_free_function_stays_one_line_each() {
    let (context, trace, candidates) = answers("overloads-mixed", "ex.run((Any)->Unit)");
    assert_eq!(candidates, 2);
    assert_golden("overloads-mixed.context-run.txt", &context);
    assert_golden("overloads-mixed.trace-run.txt", &trace);
}
