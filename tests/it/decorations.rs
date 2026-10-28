//! A call that decorates a definition (an annotation, an attribute, a decorator) carries the
//! definition in the export's call site and shows apart in `context`; an ordinary call carries
//! nothing.

use crate::common::fixture_repo;
use meada::engine::Engine;
use serde_json::Value;

const REPO: &str = "example.com/acme/decorations";

/// Every call site of the fixture: its id (relative to the repository) and the definition it
/// decorates, relative too.
fn decorations(case: &str) -> (Engine, Vec<(String, Option<String>)>) {
    let repo = fixture_repo(
        case,
        "decorations",
        Some("https://example.com/acme/decorations.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let json: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let prefix = format!("{REPO}::");
    let calls = json["calls"].as_array().unwrap();
    let calls = calls.iter().map(|c| {
        let rel = |v: &Value| {
            v.as_str()
                .map(|s| s.trim_start_matches(&prefix).to_string())
        };
        (rel(&c["id"]).unwrap(), rel(&c["decorates"]))
    });
    (engine, calls.collect())
}

#[test]
fn decorating_calls_name_the_definition_and_ordinary_calls_do_not() {
    let (_, calls) = decorations("decorations_marks");
    let marked = |id: &str, on: Option<&str>| {
        let found = calls.iter().find(|(c, _)| c == id);
        assert_eq!(
            found.map(|(_, d)| d.as_deref()),
            Some(on),
            "{id}: {calls:?}"
        );
    };
    let java = "java/UseTest.java::example.UseTest";
    marked(
        &format!("{java}::Override#0"),
        Some("java/UseTest.java::example.UseTest.toString()"),
    );
    marked(
        &format!("{java}::SuppressWarnings#0"),
        Some("java/UseTest.java::example.UseTest.t()"),
    );
    marked(&format!("{java}.t()::eq#0"), None);
    let cs = "cs/Tests.cs::Example.Tests";
    marked(
        &format!("{cs}::Fact#0"),
        Some("cs/Tests.cs::Example.Tests.Adds(int)"),
    );
    marked(
        &format!("{cs}::InlineData#0"),
        Some("cs/Tests.cs::Example.Tests.Adds(int)"),
    );
    marked(
        "kt/Tasks.kt::example.Tasks::Deprecated#0",
        Some("kt/Tasks.kt::example.Tasks.run()"),
    );
    marked("py/app.py::app.get#0", Some("py/app.py::index"));
    marked("py/app.py::index::helper#0", None);
    marked("rs/src/lib.rs::get#0", Some("rs/src/lib.rs::fast"));
    marked("rs/src/lib.rs::fast::slow#0", None);
}

#[test]
fn context_shows_annotations_apart_from_callees() {
    let (engine, _) = decorations("decorations_context");
    let text = engine
        .context("java/UseTest.java::example.UseTest", None)
        .unwrap();
    // The source span above `callers:` quotes the annotations.
    let relations = text.split_once("callers:").expect(&text).1;
    let (callees, annotations) = relations.split_once("annotations:").expect(&text);
    assert!(
        !callees.contains("Override") && !callees.contains("SuppressWarnings"),
        "{text}"
    );
    assert!(
        annotations.contains("(unresolved) Override on example.UseTest.toString()"),
        "{text}"
    );
    assert!(
        annotations.contains("(unresolved) SuppressWarnings on example.UseTest.t()"),
        "{text}"
    );
}
