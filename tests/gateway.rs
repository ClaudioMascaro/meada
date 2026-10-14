//! Gateway prefixes (spec #19): `[serve] prefix` and `[serve] strip` declare how a gateway
//! rewrites the paths of a provider, and a consumer calling the public path links through it.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{age, assert_golden, fixture_repo};
use meada::engine::Engine;

/// The consumer `gateway-client` with its two providers next to it: (client, orders, billing).
fn gateway(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    let origin = |name| Some(format!("https://example.com/acme/{name}.git"));
    (
        fixture_repo(case, "gateway-client", origin("client").as_deref()),
        fixture_repo(case, "gateway-orders", origin("orders").as_deref()),
        fixture_repo(case, "gateway-billing", origin("billing").as_deref()),
    )
}

#[test]
fn calls_through_a_gateway_path_link_inferred_and_direct_calls_keep_linking() {
    let (client, ..) = gateway("gateway_client");

    let export = Engine::open(&client).unwrap().export().unwrap();

    assert_golden("gateway-client.export.json", &export);
}

#[test]
fn the_export_carries_the_gateway_path_of_each_route() {
    let (_, _, billing) = gateway("gateway_billing");

    let export = Engine::open(&billing).unwrap().export().unwrap();

    assert_golden("gateway-billing.export.json", &export);
}

#[test]
fn a_refreshed_export_after_editing_serve_equals_a_cold_one() {
    let exports = |(client, orders, billing): &(PathBuf, PathBuf, PathBuf)| -> Vec<String> {
        [client, orders, billing]
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let edit = |billing: &PathBuf| {
        fs::write(billing.join("meada.toml"), "[serve]\nprefix = \"/pay\"\n").unwrap();
    };
    let repos = gateway("gateway_fresh");
    [&repos.0, &repos.1, &repos.2].iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit(&repos.2);

    let refreshed = exports(&repos);

    let cold = gateway("gateway_fresh_cold");
    edit(&cold.2);
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, exports(&cold));
}
