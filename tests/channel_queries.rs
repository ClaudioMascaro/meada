//! Channels in `search`, `context` and `trace` (spec #20): a channel is found by name, its
//! producers and consumers are listed across the workspace, and a side whose name Meada cannot
//! evaluate shows as unresolved and is never linked.

mod common;

use std::path::PathBuf;

use common::{assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};

/// The channels fixture as siblings in one case directory; returns `api`, whose workspace
/// holds all three.
fn workspace(case: &str) -> PathBuf {
    for name in ["worker", "billing"] {
        let origin = format!("https://example.com/channels/{name}.git");
        fixture_repo(case, &format!("channels-{name}"), Some(&origin));
    }
    fixture_repo(
        case,
        "channels-api",
        Some("https://example.com/channels/api.git"),
    )
}

fn channels(case: &str) -> Engine {
    Engine::open(&workspace(case)).unwrap()
}

#[test]
fn search_finds_a_channel_by_name_or_broker_kind() {
    let engine = channels("channel_search");

    let by_name = engine
        .search("orders.created", Some("channel"), None)
        .unwrap();
    assert_golden("channels.search-channel.txt", &by_name);
    // The broker kind matches too, and brings the unresolved sides, which have only that.
    let by_broker = engine.search("kafka", Some("channel"), None).unwrap();
    assert_golden("channels.search-channel-broker.txt", &by_broker);
    // Symbols are not channels.
    let none = engine.search("createOrder", Some("channel"), None).unwrap();
    assert_eq!(none, "no matches for `createOrder`\n");
}

#[test]
fn context_on_a_channel_lists_its_producers_and_consumers_across_repositories() {
    let engine = channels("channel_context");

    let answer = engine
        .context("channel::kafka::orders.created", None)
        .unwrap();
    assert_golden("channels.context-channel.txt", &answer);
    assert_eq!(
        engine.context("channel::kafka::nobody", None).unwrap(),
        "no channel matches `channel::kafka::nobody`\n"
    );
}

#[test]
fn context_on_an_unresolved_channel_lists_the_sides_with_no_name() {
    let engine = channels("channel_context_unresolved");

    let answer = engine.context("channel::kafka", None).unwrap();
    assert_golden("channels.context-unresolved.txt", &answer);
}

#[test]
fn context_on_a_symbol_names_the_channels_it_produces_to_and_consumes() {
    let engine = channels("channel_context_symbol");

    for (symbol, golden) in [
        ("src/orders.ts::createOrder", "producer"),
        ("src/dynamic.ts::replay", "producer-unresolved"),
        ("src/consumer.ts::handleOrder", "handler"),
        ("src/pattern.ts::seen", "handler-unresolved"),
    ] {
        let answer = engine.context(symbol, None).unwrap();
        assert_golden(&format!("channels.context-{golden}.txt"), &answer);
    }
}

#[test]
fn unresolved_channel_sides_are_never_open_items() {
    let engine = channels("channel_not_an_item");

    // `search kind=unresolved` lists open items; channel sides are not decidable in v1.
    for query in ["send", "replay", "announce", "seen", "kafka"] {
        let answer = engine.search(query, Some("unresolved"), None).unwrap();
        assert_eq!(answer, format!("no matches for `{query}`\n"));
    }
}

#[test]
fn trace_shows_an_unresolved_side_where_the_chain_ends() {
    let engine = channels("channel_trace");

    let down = engine
        .trace(Start::From("src/dynamic.ts::replay"), Direction::Down, None)
        .unwrap();
    assert_golden("channels.trace-unresolved-down.txt", &down);
    let up = engine
        .trace(Start::From("src/pattern.ts::seen"), Direction::Up, None)
        .unwrap();
    assert_golden("channels.trace-unresolved-up.txt", &up);
}

#[test]
fn the_export_keeps_the_unresolved_sides_and_links_none_of_them() {
    let api = workspace("channel_export");

    let export = Engine::open(&api.with_file_name("channels-worker"))
        .unwrap()
        .export()
        .unwrap();
    assert_golden("channels-worker.export.json", &export);
}
