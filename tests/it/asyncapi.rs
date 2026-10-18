//! AsyncAPI documents (spec #21): the channels and operations of a repository with a `[serve]`
//! table become channel sides, merged with the code sides of the same channel and direction,
//! linked to the producers and consumers of other repositories and shown by `trace`, `context`
//! and `meada status`. Over `asyncapi-events` (the documents) and `asyncapi-services` (the
//! code on the other side of them).

use crate::common;

use std::path::PathBuf;

use common::{assert_golden, fixture_repo, without_processes};
use meada::engine::{self, Direction, Engine, Start};
use serde_json::Value;

/// The fixture as siblings in one case directory; returns `services`, whose workspace holds
/// `events` too.
fn workspace(case: &str) -> PathBuf {
    let events = "https://example.com/asyncapi/events.git";
    fixture_repo(case, "asyncapi-events", Some(events));
    let services = "https://example.com/asyncapi/services.git";
    fixture_repo(case, "asyncapi-services", Some(services))
}

#[test]
fn documented_channels_are_exported_with_their_payload_shapes() {
    let services = workspace("asyncapi_export");
    let events = services.with_file_name("asyncapi-events");

    let export = Engine::open(&events).unwrap().export().unwrap();
    assert_golden("asyncapi-events.export.json", &without_processes(&export));

    let export = Engine::open(&services).unwrap().export().unwrap();
    assert_golden("asyncapi-services.export.json", &without_processes(&export));
}

#[test]
fn a_documented_side_merges_with_the_code_side_of_its_channel_and_direction() {
    let services = workspace("asyncapi_merge");
    let events = services.with_file_name("asyncapi-events");

    let export = Engine::open(&events).unwrap().export().unwrap();

    let export: Value = serde_json::from_str(&export).unwrap();
    let of = |key: &str, contract: &str| -> Vec<Value> {
        let sides = export[key].as_array().unwrap();
        let same = sides.iter().filter(|s| s["contract"] == contract);
        same.cloned().collect()
    };
    // Sent from code and documented: one side, the code's, with the document's shape.
    let updated = of("providers", "channel::kafka::orders.updated");
    assert_eq!(updated.len(), 1);
    assert!(
        updated[0]["site"]
            .as_str()
            .unwrap()
            .contains("src/emit.ts::")
    );
    assert_eq!(updated[0]["shape"]["source"]["document"], "asyncapi.yaml");
    // Received by code and documented: likewise.
    let cancelled = of("consumers", "channel::amqp::shipments.cancelled");
    assert_eq!(cancelled.len(), 1);
    assert!(
        cancelled[0]["site"]
            .as_str()
            .unwrap()
            .ends_with("src/emit.ts::onCancellation")
    );
    assert_eq!(
        cancelled[0]["shape"]["source"]["document"],
        "billing.asyncapi.yaml"
    );
}

#[test]
fn status_reports_the_documents_read_and_what_it_ignored() {
    let services = workspace("asyncapi_status");
    Engine::open(&services).unwrap().refresh().unwrap();

    assert_golden("asyncapi.status.txt", &engine::status(&services).unwrap());
}

#[test]
fn trace_down_from_a_producer_ends_at_a_documented_consumer() {
    let services = workspace("asyncapi_down");

    let answer = Engine::open(&services)
        .unwrap()
        .trace(Start::From("requestPayment"), Direction::Down, None)
        .unwrap();

    assert_golden("asyncapi.trace-payment-down.txt", &answer);
}

#[test]
fn trace_up_from_a_handler_ends_at_a_documented_producer() {
    let services = workspace("asyncapi_up");

    let answer = Engine::open(&services)
        .unwrap()
        .trace(Start::From("onInvoice"), Direction::Up, None)
        .unwrap();

    assert_golden("asyncapi.trace-invoice-up.txt", &answer);
}

#[test]
fn trace_reaches_the_handler_of_a_merged_side() {
    let services = workspace("asyncapi_merged_trace");

    let answer = Engine::open(&services)
        .unwrap()
        .trace(Start::From("cancel"), Direction::Down, None)
        .unwrap();

    assert_golden("asyncapi.trace-cancel-down.txt", &answer);
}

#[test]
fn context_on_a_channel_shows_a_documented_side_and_its_payload_fields() {
    let services = workspace("asyncapi_context");
    let engine = Engine::open(&services).unwrap();

    let created = engine
        .context("channel::kafka::orders.created", None)
        .unwrap();
    let payments = engine
        .context("channel::kafka::payments.requested", None)
        .unwrap();
    let invoices = engine
        .context("channel::amqp::invoices.issued", None)
        .unwrap();

    assert_golden(
        "asyncapi.context-channels.txt",
        &format!("{created}\n{payments}\n{invoices}"),
    );
}

#[test]
fn search_lists_asyncapi_documents_with_their_channel_count() {
    let services = workspace("asyncapi_search_documents");

    let answer = Engine::open(&services)
        .unwrap()
        .search("asyncapi", Some("document"), None)
        .unwrap();

    assert_golden("asyncapi.search-documents.txt", &answer);
}

#[test]
fn search_finds_a_documented_channel() {
    let services = workspace("asyncapi_search");

    let answer = Engine::open(&services)
        .unwrap()
        .search("shipments", Some("channel"), None)
        .unwrap();

    assert_golden("asyncapi.search-shipments.txt", &answer);
}
