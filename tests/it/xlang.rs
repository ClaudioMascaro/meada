//! A TypeScript repository and a Python one that call each other (spec #24): the storefront
//! reaches the inventory service through a base URL variable and through a declared host, the
//! inventory reaches the storefront the same two ways, with the base read from the environment
//! in every form Python has. `trace` follows both directions across both boundaries.

use crate::common;

use std::fs;

use common::{assert_golden, without_processes, xlang};
use meada::engine::{Direction, Engine, Start};

#[test]
fn both_repositories_link_to_each_other_through_a_variable_and_a_host() {
    let (storefront, inventory) = xlang("xlang_export");

    for (repo, name) in [(storefront, "storefront"), (inventory, "inventory")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(
            &format!("xlang-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

#[test]
fn two_exports_of_the_cross_language_fixture_are_byte_identical() {
    let export = |case| {
        let (storefront, inventory) = xlang(case);
        [storefront, inventory].map(|r| Engine::open(&r).unwrap().export().unwrap())
    };

    assert_eq!(export("xlang_twice_a"), export("xlang_twice_b"));
}

#[test]
fn context_of_a_python_function_shows_its_linked_and_unresolved_consumers() {
    let (_, inventory) = xlang("xlang_context");

    let answer = Engine::open(&inventory)
        .unwrap()
        .context("restock", None)
        .unwrap();

    assert_golden("xlang.context-restock.txt", &answer);
}

#[test]
fn trace_down_crosses_into_python_and_back_to_typescript() {
    let (storefront, _) = xlang("xlang_down");

    let answer = Engine::open(&storefront)
        .unwrap()
        .trace(Start::From("syncInventory"), Direction::Down, None)
        .unwrap();

    assert_golden("xlang.trace-down.txt", &answer);
}

/// The storefront produces the Kafka topic `stock.restocked` in TypeScript; the inventory
/// service consumes it in Python, the function that subscribes being the handler (spec #73).
#[test]
fn trace_down_from_a_typescript_producer_reaches_the_python_handler_of_the_topic() {
    let (storefront, _) = xlang("xlang_channel_down");

    let answer = Engine::open(&storefront)
        .unwrap()
        .trace(Start::From("announceRestock"), Direction::Down, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[exact: channel::kafka::stock.restocked] example.com/xlang/inventory \
             inventory/events.py:10 consume_restocks"
        ),
        "{answer}"
    );
    assert_golden("xlang.trace-channel-down.txt", &answer);
}

#[test]
fn trace_up_crosses_into_python_and_back_to_typescript() {
    let (storefront, _) = xlang("xlang_up");

    let answer = Engine::open(&storefront)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/prices/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("xlang.trace-up.txt", &answer);
}

#[test]
fn trace_from_uncommitted_changes_starts_at_an_edited_python_file() {
    let (_, inventory) = xlang("xlang_uncommitted");
    let main = inventory.join("inventory/main.py");
    let edited = fs::read_to_string(&main)
        .unwrap()
        .replace("def reserve():", "def reserve(quantity=1):");
    fs::write(&main, edited).unwrap();

    let answer = Engine::open(&inventory)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("xlang.trace-uncommitted.txt", &answer);
}
