//! A TypeScript repository and a Python one that call each other (spec #24): the storefront
//! reaches the inventory service through a base URL variable and through a declared host, the
//! inventory reaches the storefront the same two ways, with the base read from the environment
//! in every form Python has. `trace` follows both directions across both boundaries.

use crate::common;

use std::fs;

use common::{assert_golden, without_processes, xlang, xlang_go, xlang_java, xlang_rust};
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

/// The same chain with a Rust service (spec #30): the web app (TypeScript, Next.js) reaches the
/// api (axum) through `API_URL` and through the api's declared host; the api reaches the web
/// app with reqwest through `WEB_URL`, read from the environment, and through its declared host.
#[test]
fn rust_and_typescript_repositories_link_to_each_other_through_a_variable_and_a_host() {
    let (web, api) = xlang_rust("xlang_rust_export");

    for (repo, name) in [(web, "web"), (api, "api")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(
            &format!("xlang-rust-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

#[test]
fn two_exports_of_the_rust_cross_language_fixture_are_byte_identical() {
    let export = |case| {
        let (web, api) = xlang_rust(case);
        [web, api].map(|r| Engine::open(&r).unwrap().export().unwrap())
    };

    assert_eq!(export("xlang_rust_twice_a"), export("xlang_rust_twice_b"));
}

#[test]
fn trace_down_crosses_into_rust_and_back_to_typescript() {
    let (web, _) = xlang_rust("xlang_rust_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("syncOrders"), Direction::Down, None)
        .unwrap();

    assert_golden("xlang-rust.trace-down.txt", &answer);
}

/// The web app produces the Kafka topic `orders.shipped` in TypeScript (kafkajs); the api
/// service consumes it in Rust (rdkafka), the function that subscribes being the handler
/// (spec #30).
#[test]
fn trace_down_from_a_typescript_producer_reaches_the_rust_handler_of_the_topic() {
    let (web, _) = xlang_rust("xlang_rust_channel_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("announceShipment"), Direction::Down, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[exact: channel::kafka::orders.shipped] example.com/xlang-rust/api \
             src/events.rs:9 consume_shipments"
        ),
        "{answer}"
    );
    assert_golden("xlang-rust.trace-channel-down.txt", &answer);
}

/// The web app produces the Kafka topic `orders.shipped` in TypeScript (kafkajs); the api
/// service consumes it in Go (sarama), the `ConsumeClaim` of the handler type being the handler
/// (spec #25, #82).
#[test]
fn trace_down_from_a_typescript_producer_reaches_the_go_handler_of_the_topic() {
    let (web, _) = xlang_go("xlang_go_channel_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("announceShipment"), Direction::Down, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[exact: channel::kafka::orders.shipped] example.com/xlang-go/api \
             events.go:14 shipments.ConsumeClaim"
        ),
        "{answer}"
    );
    assert_golden("xlang-go.trace-channel-down.txt", &answer);
}

#[test]
fn trace_up_crosses_into_rust_and_back_to_typescript() {
    let (web, _) = xlang_rust("xlang_rust_up");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/prices/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("xlang-rust.trace-up.txt", &answer);
}

#[test]
fn trace_from_uncommitted_changes_starts_at_an_edited_rust_file() {
    let (_, api) = xlang_rust("xlang_rust_uncommitted");
    let orders = api.join("src/orders.rs");
    let edited = fs::read_to_string(&orders)
        .unwrap()
        .replace("async fn reserve() {", "async fn reserve(_body: String) {");
    fs::write(&orders, edited).unwrap();

    let answer = Engine::open(&api)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("xlang-rust.trace-uncommitted.txt", &answer);
}

/// The same chain with a Go service (spec #25): the web app (TypeScript, Next.js) reaches the api
/// (net/http) through `API_URL` and through the api's declared host; the api reaches the web app
/// with net/http and resty through `WEB_URL` and `STOCK_URL`, read from the environment, and
/// through its declared host.
#[test]
fn go_and_typescript_repositories_link_to_each_other_through_a_variable_and_a_host() {
    let (web, api) = xlang_go("xlang_go_export");

    for (repo, name) in [(web, "web"), (api, "api")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(
            &format!("xlang-go-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

/// Every shape of environment read the Go pack folds, and the near misses it leaves alone: the
/// `via` of the request each function of `go-env-reads` makes.
#[test]
fn a_go_base_url_is_the_variable_of_exactly_one_environment_read() {
    let repo = common::fixture_repo(
        "xlang_go_env_reads",
        "go-env-reads",
        Some("https://example.com/goenv/probes.git"),
    );
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let vias: std::collections::BTreeMap<&str, Option<&str>> = export["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let site = c["site"].as_str().unwrap();
            let function = site.split("::").nth(2).unwrap();
            (function, c["via"].as_str())
        })
        .collect();

    let read = [
        ("direct", "DIRECT_URL"),
        ("named", "NAMED_URL"),
        ("fallback", "FALLBACK_URL"),
        ("constantDefault", "CONST_URL"),
        ("emptyOnTheLeft", "LEFT_URL"),
        ("selfTrim", "SELF_URL"),
        ("trimmed", "TRIMMED_URL"),
        ("trimmedRight", "RIGHT_URL"),
        ("lookedUp", "LOOKUP_URL"),
        ("lookedUpFallback", "OK_URL"),
        ("lookedUpOrEmpty", "OK_EMPTY_URL"),
        ("lengthFallback", "LEN_URL"),
        ("formatted", "FORMAT_URL"),
        ("packageLevel", "PKG_URL"),
    ];
    for (function, variable) in read {
        let expected = format!("env:{variable}");
        assert_eq!(
            vias.get(function),
            Some(&Some(expected.as_str())),
            "{function}"
        );
    }
    let unread = [
        "twoReads",
        "trimmedByARead",
        "fallbackToAnotherRead",
        "fallbackToANamedRead",
        "fallbackToACall",
        "trimmedAfterANamedRead",
        "fallbackToALoopVariable",
        "fallbackToAVariable",
        "fallbackWithElse",
        "fallbackOnAnotherTest",
        "assignedAgain",
        "fallbackOfALiteral",
        "hostInTheMiddle",
        "nameFromAVariable",
        "trimmedLiteral",
        "trimmedOtherName",
        "shadowedPackage",
        "okOfAnotherLookup",
        "okOfNoLookup",
        "okAssignedAgain",
        "lengthShadowed",
        "lookedUpToAVariable",
    ];
    for function in unread {
        assert_eq!(vias.get(function), Some(&None), "{function}");
    }
    assert_eq!(vias.len(), read.len() + unread.len());
}

#[test]
fn two_exports_of_the_go_cross_language_fixture_are_byte_identical() {
    let export = |case| {
        let (web, api) = xlang_go(case);
        [web, api].map(|r| Engine::open(&r).unwrap().export().unwrap())
    };

    assert_eq!(export("xlang_go_twice_a"), export("xlang_go_twice_b"));
}

#[test]
fn trace_down_crosses_into_go_and_back_to_typescript() {
    let (web, _) = xlang_go("xlang_go_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("syncOrders"), Direction::Down, None)
        .unwrap();

    assert_golden("xlang-go.trace-down.txt", &answer);
}

#[test]
fn trace_up_crosses_into_go_and_back_to_typescript() {
    let (web, _) = xlang_go("xlang_go_up");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/prices/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("xlang-go.trace-up.txt", &answer);
}

#[test]
fn trace_from_uncommitted_changes_starts_at_an_edited_go_file() {
    let (_, api) = xlang_go("xlang_go_uncommitted");
    let orders = api.join("orders.go");
    let edited = fs::read_to_string(&orders)
        .unwrap()
        .replace("func reserve(w", "func reserve(_ int, w");
    fs::write(&orders, edited).unwrap();

    let answer = Engine::open(&api)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("xlang-go.trace-uncommitted.txt", &answer);
}

/// The same chain with a Java service (spec #26): the web app (TypeScript, Next.js) reaches the
/// api (Spring MVC) through `API_URL` and through the api's declared host; the api reaches the
/// web app with RestTemplate, WebClient and `java.net.http` through bases its Spring
/// configuration gives `@Value` fields and a constructor parameter (a placeholder, a literal
/// URL, a placeholder with a default a peer serves and one no peer serves) and through
/// `System.getenv`.
#[test]
fn java_and_typescript_repositories_link_to_each_other_through_a_variable_and_a_host() {
    let (web, api) = xlang_java("xlang_java_export");

    for (repo, name) in [(web, "web"), (api, "api")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(
            &format!("xlang-java-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

#[test]
fn two_exports_of_the_java_cross_language_fixture_are_byte_identical() {
    let export = |case| {
        let (web, api) = xlang_java(case);
        [web, api].map(|r| Engine::open(&r).unwrap().export().unwrap())
    };

    assert_eq!(export("xlang_java_twice_a"), export("xlang_java_twice_b"));
}

#[test]
fn trace_down_crosses_into_java_and_back_to_typescript() {
    let (web, _) = xlang_java("xlang_java_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("syncOrders"), Direction::Down, None)
        .unwrap();

    assert_golden("xlang-java.trace-down.txt", &answer);
}

/// The web app produces the Kafka topic `orders.shipped` in TypeScript (kafkajs); the api
/// service consumes it with a Spring `@KafkaListener` whose topic its configuration names, the
/// annotated method being the handler (spec #26, #92).
#[test]
fn trace_down_from_a_typescript_producer_reaches_the_java_kafka_listener() {
    let (web, _) = xlang_java("xlang_java_channel_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("announceShipment"), Direction::Down, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[exact: channel::kafka::orders.shipped] example.com/xlang-java/api \
             src/main/java/com/example/api/ShipmentListener.java:7 \
             com.example.api.ShipmentListener.onShipment(String)"
        ),
        "{answer}"
    );
    assert_golden("xlang-java.trace-channel-down.txt", &answer);
}

#[test]
fn trace_up_crosses_into_java_and_back_to_typescript() {
    let (web, _) = xlang_java("xlang_java_up");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/prices/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("xlang-java.trace-up.txt", &answer);
}

/// A route a Feign client calls: up from the route, through the interface method (a consumer at
/// a definition), to the caller of the method and on to the web app that reaches it.
#[test]
fn trace_up_from_a_route_goes_through_the_feign_interface_method() {
    let (web, _) = xlang_java("xlang_java_up_feign");
    let up = |route: &str| {
        Engine::open(&web)
            .unwrap()
            .trace(Start::From(route), Direction::Up, None)
            .unwrap()
    };

    assert_golden(
        "xlang-java.trace-up-feign.txt",
        &up("http::GET::/api/inventory/{}"),
    );
    // Without `url` the host is unresolved and the client's name is in the reason.
    let catalog = up("http::GET::/api/catalog");
    assert!(
        catalog.contains("host unresolved (Feign client catalog); the only repository serving it is example.com/xlang-java/web"),
        "{catalog}"
    );
    assert!(catalog.contains("CatalogClient.all()"), "{catalog}");
}

#[test]
fn trace_from_uncommitted_changes_starts_at_an_edited_java_file() {
    let (_, api) = xlang_java("xlang_java_uncommitted");
    let controller = api.join("src/main/java/com/example/api/OrderController.java");
    let edited = fs::read_to_string(&controller).unwrap().replace(
        "status(@PathVariable String id)",
        "status(@PathVariable String id, int at)",
    );
    fs::write(&controller, edited).unwrap();

    let answer = Engine::open(&api)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("xlang-java.trace-uncommitted.txt", &answer);
}

/// The default's host two repositories serve (a mirror of the web app beside it): the one
/// reading that applies links ambiguous, and it is one open item, the variable's reading gone.
#[test]
fn a_default_whose_host_two_repositories_serve_is_one_open_item() {
    let (web, api) = xlang_java("xlang_java_ambiguous_default");
    let mirror = web.parent().unwrap().join("xlang-java-mirror");
    let _ = fs::remove_dir_all(&mirror);
    fs::create_dir_all(mirror.join("app/api/audit")).unwrap();
    fs::write(
        mirror.join("meada.toml"),
        "[serve]\nhosts = [\"web.example.com\"]\n",
    )
    .unwrap();
    let route = "app/api/audit/route.ts";
    fs::copy(web.join(route), mirror.join(route)).unwrap();
    common::git(&mirror, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let origin = "https://example.com/xlang-java/mirror.git";
    common::git(&mirror, &["remote", "add", "origin", origin]);
    common::git(&mirror, &["add", "-A"]);
    common::git(&mirror, &["commit", "-q", "-m", "fixture"]);
    let toml = api.join("meada.toml");
    let peer = "\n[[peer]]\nrepo = \"example.com/xlang-java/mirror\"\n";
    fs::write(&toml, fs::read_to_string(&toml).unwrap() + peer).unwrap();

    let items = Engine::open(&api).unwrap().open_items().unwrap();

    let audit: Vec<_> = (items.iter())
        .filter(|i| i.path.ends_with("AuditClient.java"))
        .collect();
    assert_eq!(
        audit.len(),
        1,
        "{:?}",
        items.iter().map(|i| &i.id).collect::<Vec<_>>()
    );
    assert_eq!(audit[0].candidates.len(), 2);
}
