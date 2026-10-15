//! Gateway prefixes (spec #19): `[serve] prefix` and `[serve] strip` declare how a gateway
//! rewrites the paths of a provider, and a consumer calling the public path links through it.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{age, assert_golden, fixture_repo};
use meada::engine::Engine;

/// The consumer `gateway-client` with its providers next to it: (client, orders, billing). The
/// ledger, a third provider, is their sibling too.
fn gateway(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    let origin = |name| Some(format!("https://example.com/acme/{name}.git"));
    fixture_repo(case, "gateway-ledger", origin("ledger").as_deref());
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

#[test]
fn within_one_repository_the_code_path_wins_over_a_gateway_path() {
    let (client, ..) = gateway("gateway_code_wins");

    let export = Engine::open(&client).unwrap().export().unwrap();

    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    let links: Vec<_> = (export["links"].as_array().unwrap().iter())
        .filter(|l| l["consumer"] == "example.com/acme/client::src/client.ts::ledger::fetch#0")
        .collect();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(
        links[0]["provider"],
        "example.com/acme/ledger::app/ledger/route.ts::GET"
    );
    assert_eq!(links[0]["level"], "inferred");
}

#[test]
fn a_dynamic_url_fits_a_route_by_its_gateway_path_with_the_route_s_own_contract() {
    let (client, ..) = gateway("gateway_dynamic");

    let items = Engine::open(&client).unwrap().open_items().unwrap();

    let order = (items.iter())
        .find(|i| i.id == "example.com/acme/client::src/client.ts::order::fetch#0")
        .expect("the dynamic URL is an open item");
    let candidates: Vec<_> = (order.candidates.iter())
        .map(|c| format!("{} {}", c.contract, c.site))
        .collect();
    assert_eq!(
        candidates,
        ["http::GET::/orders/{} example.com/acme/orders::app/orders/[id]/route.ts::GET"]
    );
}

#[test]
fn a_decision_for_the_provider_behind_a_colliding_gateway_path_applies() {
    let (client, ..) = gateway("gateway_decide");
    let audit = "example.com/acme/client::src/client.ts::audit::fetch#0";

    // The candidates are sorted by repository: billing, behind its gateway prefix, is first.
    let (_, stderr, ok) = common::meada(&client, &["decide", "--item", audit], "1\n", |c| {
        c.env("LANG", "en_US.UTF-8");
    });

    assert!(ok, "{stderr}");
    let written = common::written(&client).expect("the file is written");
    assert!(
        written.contains("\"example.com/acme/billing::http::GET::/audit\""),
        "{written}"
    );
    let export = Engine::open(&client).unwrap().export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    assert_eq!(export.get("stale_decisions"), None, "{export}");
    let links: Vec<_> = (export["links"].as_array().unwrap().iter())
        .filter(|l| l["consumer"] == audit)
        .collect();
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0]["level"], "decided");
    assert_eq!(
        links[0]["provider"],
        "example.com/acme/billing::app/audit/route.ts::GET"
    );
}
