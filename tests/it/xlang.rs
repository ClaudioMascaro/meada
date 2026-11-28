use crate::common;

use std::fs;

use common::{
    assert_golden, without_processes, xlang, xlang_csharp, xlang_go, xlang_java, xlang_java_mirror,
    xlang_kotlin, xlang_rust, xlang_swift,
};
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

#[test]
fn go_and_typescript_repositories_link_to_each_other_through_a_variable_and_a_host() {
    let (web, api) = xlang_go("xlang_go_export");

    for (repo, name) in [(web, "web"), (api, "api")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        if name == "api" {
            assert!(
                export.contains("tags.go::stockByTag::http.Get#0")
                    && export.contains("env:STOCK_URL")
            );
        }
        assert_golden(
            &format!("xlang-go-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

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
        ("tagNested", "TAG_API_URL"),
        ("tagEnvconfig", "TAG_ENVCONFIG_URL"),
        ("tagOptions", "TAG_CAARLO_URL"),
        ("Client.OrdersDirect", "FIELD_URL"),
        ("Client.Orders", "FIELD_URL"),
        ("Cross.Get", "CROSS_URL"),
        ("Typed.Get", "CROSS_URL"),
        ("TwinA.Get", "TWIN_A_URL"),
        ("TwinB.Get", "TWIN_B_URL"),
        ("AnonSvc.Get", "ANON_URL"),
        ("AnonLocal", "ANON_BUS_URL"),
        ("AnonLoaded", "ANON_URL"),
        ("AnonNamed", "ANON_BROKER_URL"),
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
        "tagUntagged",
        "twoLiterals.get",
        "twoCallers.get",
        "byPosition.get",
        "untold.get",
        "unset.get",
        "assigned.get",
        "uncalled.get",
        "matched.get",
        "literal.get",
        "Unknown.Get",
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

#[test]
fn kotlin_and_typescript_repositories_link_to_each_other_through_a_variable_and_a_host() {
    let (web, api) = xlang_kotlin("xlang_kotlin_export");

    for (repo, name) in [(web, "web"), (api, "api")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(
            &format!("xlang-kotlin-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

#[test]
fn two_exports_of_the_kotlin_cross_language_fixture_are_byte_identical() {
    let export = |case| {
        let (web, api) = xlang_kotlin(case);
        [web, api].map(|r| Engine::open(&r).unwrap().export().unwrap())
    };

    assert_eq!(
        export("xlang_kotlin_twice_a"),
        export("xlang_kotlin_twice_b")
    );
}

#[test]
fn trace_down_crosses_into_kotlin_and_back_to_typescript() {
    let (web, _) = xlang_kotlin("xlang_kotlin_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("syncOrders"), Direction::Down, None)
        .unwrap();

    assert_golden("xlang-kotlin.trace-down.txt", &answer);
}

#[test]
fn trace_down_from_a_typescript_producer_reaches_the_kotlin_kafka_listener() {
    let (web, _) = xlang_kotlin("xlang_kotlin_channel_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("announceShipment"), Direction::Down, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[exact: channel::kafka::orders.shipped] example.com/xlang-kotlin/api \
             src/main/kotlin/com/example/api/ShipmentListener.kt:7 \
             com.example.api.ShipmentListener.onShipment(String)"
        ),
        "{answer}"
    );
    assert_golden("xlang-kotlin.trace-channel-down.txt", &answer);
}

#[test]
fn trace_up_crosses_into_kotlin_and_back_to_typescript() {
    let (web, _) = xlang_kotlin("xlang_kotlin_up");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/prices/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("xlang-kotlin.trace-up.txt", &answer);
}

#[test]
fn trace_from_uncommitted_changes_starts_at_an_edited_kotlin_file() {
    let (_, api) = xlang_kotlin("xlang_kotlin_uncommitted");
    let server = api.join("src/main/kotlin/com/example/api/Server.kt");
    let edited = fs::read_to_string(&server)
        .unwrap()
        .replace("stock.level()", "stock.level(); println(id)");
    fs::write(&server, edited).unwrap();

    let answer = Engine::open(&api)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("xlang-kotlin.trace-uncommitted.txt", &answer);
}

#[test]
fn csharp_and_typescript_repositories_link_to_each_other_through_a_variable_and_a_host() {
    let (web, api) = xlang_csharp("xlang_csharp_export");

    for (repo, name) in [(web, "web"), (api, "api")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(
            &format!("xlang-csharp-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

#[test]
fn two_exports_of_the_csharp_cross_language_fixture_are_byte_identical() {
    let export = |case| {
        let (web, api) = xlang_csharp(case);
        [web, api].map(|r| Engine::open(&r).unwrap().export().unwrap())
    };

    assert_eq!(
        export("xlang_csharp_twice_a"),
        export("xlang_csharp_twice_b")
    );
}

#[test]
fn trace_down_crosses_into_csharp_and_back_to_typescript() {
    let (web, _) = xlang_csharp("xlang_csharp_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("syncOrders"), Direction::Down, None)
        .unwrap();

    for link in [
        "→[inferred: base URL from API_URL, a base_url_env of peer example.com/xlang-csharp/api]",
        "→[exact: host api.example.com is served by example.com/xlang-csharp/api]",
        "→[inferred: base URL from Prices__BaseUrl, a base_url_env of peer example.com/xlang-csharp/web] example.com/xlang-csharp/web app/api/prices/[sku]/route.ts:1 GET",
        "→[inferred: base URL from WEB_URL, a base_url_env of peer example.com/xlang-csharp/web] example.com/xlang-csharp/web app/api/stock/route.ts:1 GET",
        "→[exact: host web.example.com is served by example.com/xlang-csharp/web] example.com/xlang-csharp/web app/api/audit/route.ts:1 POST",
    ] {
        assert!(answer.contains(link), "{link}\n{answer}");
    }
    assert_golden("xlang-csharp.trace-down.txt", &answer);
}

#[test]
fn trace_down_from_a_csharp_typed_client_reaches_the_typescript_provider() {
    let (_, api) = xlang_csharp("xlang_csharp_typed");

    let answer = Engine::open(&api)
        .unwrap()
        .trace(
            Start::From("Api.Clients.CatalogClient.Products()"),
            Direction::Down,
            None,
        )
        .unwrap();

    let link = "→[exact: host web.example.com is served by example.com/xlang-csharp/web] \
                example.com/xlang-csharp/web app/api/catalog/route.ts:1 GET";
    assert!(answer.contains(link), "{answer}");
    assert_golden("xlang-csharp.trace-typed-client.txt", &answer);
}

#[test]
fn trace_down_from_a_typescript_producer_reaches_the_csharp_kafka_consumer() {
    let (web, _) = xlang_csharp("xlang_csharp_channel_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("announceShipment"), Direction::Down, None)
        .unwrap();

    assert!(
        answer.contains(
            "→[exact: channel::kafka::orders.shipped] example.com/xlang-csharp/api \
             Messaging/ShipmentListener.cs:16 Api.Messaging.ShipmentListener.OnShipment()"
        ),
        "{answer}"
    );
    assert_golden("xlang-csharp.trace-channel-down.txt", &answer);
}

#[test]
fn trace_up_crosses_into_csharp_and_back_to_typescript() {
    let (web, _) = xlang_csharp("xlang_csharp_up");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/prices/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("xlang-csharp.trace-up.txt", &answer);
}

#[test]
fn trace_from_uncommitted_changes_starts_at_an_edited_csharp_file() {
    let (_, api) = xlang_csharp("xlang_csharp_uncommitted");
    let controller = api.join("Controllers/OrdersController.cs");
    let edited = fs::read_to_string(&controller).unwrap().replace(
        "return await _prices.PriceOf(id);",
        "Console.WriteLine(id);\n        return await _prices.PriceOf(id);",
    );
    fs::write(&controller, edited).unwrap();

    let answer = Engine::open(&api)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("xlang-csharp.trace-uncommitted.txt", &answer);
}

#[test]
fn swift_and_typescript_repositories_link_to_each_other_through_a_variable_and_a_host() {
    let (web, api) = xlang_swift("xlang_swift_export");

    for (repo, name) in [(web, "web"), (api, "api")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(
            &format!("xlang-swift-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

#[test]
fn a_swift_base_url_is_the_variable_of_exactly_one_process_environment_read() {
    let repo = common::fixture_repo(
        "xlang_swift_env_reads",
        "swift-env-reads",
        Some("https://example.com/swiftenv/probes.git"),
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
            (function.trim_end_matches("()"), c["via"].as_str())
        })
        .collect();

    let read = [
        ("direct", "DIRECT_URL"),
        ("named", "NAMED_URL"),
        ("fallback", "FALLBACK_URL"),
        ("emptyFallback", "EMPTY_URL"),
        ("trimmed", "TRIMMED_URL"),
        ("optionalTrim", "OPTIONAL_URL"),
        ("replaced", "REPLACED_URL"),
        ("typeMember", "MEMBER_URL"),
        ("wrapped", "WRAPPED_URL"),
        ("lowered", "LOWERED_URL"),
        ("qualified", "QUALIFIED_URL"),
        ("interpolated", "INTERPOLATED_URL"),
        ("topLevel", "PKG_URL"),
        ("typeLevel", "STATIC_URL"),
        ("optionalBinding", "BINDING_URL"),
        ("guardLet", "GUARD_URL"),
        ("guardWithCondition", "COND_URL"),
        ("guardFallback", "GUARD_FALLBACK_URL"),
        ("nameFromAVariable", "KEY_URL"),
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
        "fallbackToAnotherRead",
        "fallbackToACall",
        "fallbackToAName",
        "fallbackInterpolated",
        "trimmedByARead",
        "replacedByALocal",
        "methodOfAnotherKind",
        "assignedAgain",
        "vaporEnvironment",
        "ofAnotherObject",
        "guardVar",
        "guardVapor",
        "shadowed",
    ];
    for function in unread {
        assert_eq!(vias.get(function), Some(&None), "{function}");
    }
    assert_eq!(vias.len(), read.len() + unread.len(), "{vias:?}");
}

#[test]
fn two_exports_of_the_swift_cross_language_fixture_are_byte_identical() {
    let export = |case| {
        let (web, api) = xlang_swift(case);
        [web, api].map(|r| Engine::open(&r).unwrap().export().unwrap())
    };

    assert_eq!(export("xlang_swift_twice_a"), export("xlang_swift_twice_b"));
}

#[test]
fn trace_down_crosses_into_swift_and_back_to_typescript() {
    let (web, _) = xlang_swift("xlang_swift_down");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("syncOrders"), Direction::Down, None)
        .unwrap();

    for link in [
        "→[inferred: base URL from API_URL, a base_url_env of peer example.com/xlang-swift/api]",
        "→[exact: host api.example.com is served by example.com/xlang-swift/api]",
        "→[inferred: base URL from WEB_URL, a base_url_env of peer example.com/xlang-swift/web] example.com/xlang-swift/web app/api/prices/[sku]/route.ts:1 GET",
        "→[inferred: base URL from STOCK_URL, a base_url_env of peer example.com/xlang-swift/web] example.com/xlang-swift/web app/api/stock/route.ts:1 GET",
        "→[inferred: base URL from WEB_URL, a base_url_env of peer example.com/xlang-swift/web] example.com/xlang-swift/web app/api/audit/route.ts:1 POST",
        "→[exact: host web.example.com is served by example.com/xlang-swift/web] example.com/xlang-swift/web app/api/promotions/route.ts:1 GET",
    ] {
        assert!(answer.contains(link), "{link}\n{answer}");
    }
    assert_golden("xlang-swift.trace-down.txt", &answer);
}

#[test]
fn trace_up_crosses_into_swift_and_back_to_typescript() {
    let (web, _) = xlang_swift("xlang_swift_up");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("http::GET::/api/prices/{}"),
            Direction::Up,
            None,
        )
        .unwrap();

    assert_golden("xlang-swift.trace-up.txt", &answer);
}

#[test]
fn trace_from_uncommitted_changes_starts_at_an_edited_swift_file() {
    let (_, api) = xlang_swift("xlang_swift_uncommitted");
    let orders = api.join("Sources/Orders.swift");
    let edited = fs::read_to_string(&orders).unwrap().replace(
        "    try await promotions()\n",
        "    req.logger.info(\"reserve\")\n    try await promotions()\n",
    );
    fs::write(&orders, edited).unwrap();

    let answer = Engine::open(&api)
        .unwrap()
        .trace(Start::Uncommitted, Direction::Up, None)
        .unwrap();

    assert_golden("xlang-swift.trace-uncommitted.txt", &answer);
}

#[test]
fn a_default_whose_host_two_repositories_serve_is_one_open_item() {
    let (_, api) = xlang_java_mirror("xlang_java_ambiguous_default", &["app/api/audit/route.ts"]);

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

#[test]
fn a_declarative_client_method_is_an_open_item_at_its_definition() {
    let (_, api) = xlang_java_mirror("xlang_java_open_declarative", &["app/api/catalog/route.ts"]);
    let dynamic = "package com.example.api;\n\nimport retrofit2.http.GET;\nimport retrofit2.http.Url;\n\n\
                   public interface DynamicClient {\n    @GET\n    String fetch(@Url String url);\n}\n";
    fs::write(
        api.join("src/main/java/com/example/api/DynamicClient.java"),
        dynamic,
    )
    .unwrap();
    common::git(&api, &["add", "-A"]);
    common::git(&api, &["commit", "-q", "-m", "dynamic"]);
    let engine = Engine::open(&api).unwrap();

    let items = engine.local_items().unwrap();

    let catalog = "example.com/xlang-java/api::src/main/java/com/example/api/CatalogClient.java::com.example.api.CatalogClient.all()";
    let fetch = "example.com/xlang-java/api::src/main/java/com/example/api/DynamicClient.java::com.example.api.DynamicClient.fetch(String)";
    let item = |id: &str| {
        items
            .iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    let (ambiguous, unresolved) = (item(catalog), item(fetch));
    assert_eq!(ambiguous.kind.name(), "ambiguous");
    assert_eq!(ambiguous.candidates.len(), 2);
    assert_eq!(
        ambiguous.location(),
        "src/main/java/com/example/api/CatalogClient.java:10"
    );
    assert_eq!(
        ambiguous.expression,
        "@GetMapping(\"/api/catalog\") String all();"
    );
    let enclosing = ambiguous.enclosing.as_ref().unwrap();
    assert_eq!(enclosing.name, "com.example.api.CatalogClient.all()");
    assert_eq!(unresolved.kind.name(), "unresolved");
    assert_eq!(unresolved.expression, "@GET String fetch(@Url String url);");
    let search = |kind| engine.search("", Some(kind), None).unwrap();
    assert!(
        search("ambiguous").contains(&format!(
            "CatalogClient.java:10 (ambiguous) candidates: 2; run `meada decide --item {catalog}`"
        )),
        "{}",
        search("ambiguous")
    );
    assert!(
        search("unresolved").contains("DynamicClient.java:7 (unresolved)"),
        "{}",
        search("unresolved")
    );
    assert!(
        engine
            .search("fetch", Some("unresolved"), None)
            .unwrap()
            .contains(fetch)
    );
    let (list, stderr, ok) = common::meada(&api, &["decide", "--list"], "", |c| {
        c.env("LANG", "en_US.UTF-8");
    });
    assert!(ok, "{stderr}");
    assert!(list.contains(&format!("{catalog}\nsrc/main/java/com/example/api/CatalogClient.java:10  @GetMapping(\"/api/catalog\") String all();\n  kind = \"ambiguous-link\"\n  1) ")), "{list}");
    assert!(list.contains(fetch), "{list}");
}

#[test]
fn a_backstage_config_read_is_the_variable_its_app_config_value_names() {
    let case = "xlang_backstage";
    let web = common::fixture_repo(
        case,
        "ts-backstage-web",
        Some("https://example.com/backstage/web.git"),
    );
    for (fixture, origin) in [
        ("ts-backstage-api", "https://example.com/backstage/api.git"),
        (
            "ts-backstage-other",
            "https://example.com/backstage/other.git",
        ),
    ] {
        common::fixture_repo(case, fixture, Some(origin));
    }
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&web).unwrap().export().unwrap()).unwrap();
    let mut vias = std::collections::BTreeMap::<_, Vec<_>>::new();
    for consumer in export["consumers"].as_array().unwrap() {
        let site = consumer["site"].as_str().unwrap();
        vias.entry(site.split("::").nth(2).unwrap())
            .or_default()
            .push(consumer["via"].as_str());
    }
    let expected: &[(&str, &[Option<&str>])] = &[
        (
            "viaGetString",
            &[Some("env:API_URL"), Some("host:localhost:7007")],
        ),
        ("viaOptional", &[Some("env:OPT_URL")]),
        ("viaLocalFile", &[Some("env:LOCAL_URL")]),
        ("viaIncludedKey", &[Some("env:API_URL")]),
        ("viaIncludedBlock", &[Some("env:API_URL")]),
        ("viaEnvValue", &[Some("env:API_URL")]),
        ("literalValue", &[Some("host:other.example.com")]),
        ("missingKey", &[None]),
        ("notAppConfig", &[None]),
        (
            "OrdersClient.list",
            &[Some("env:API_URL"), Some("host:localhost:7007")],
        ),
        ("PublicClient.list", &[None]),
        ("TwiceClient.list", &[None]),
        ("ConditionalClient.list", &[None]),
        ("LateClient.list", &[None]),
        (
            "ParameterClient.list",
            &[Some("env:API_URL"), Some("host:localhost:7007")],
        ),
        (
            "AssignedClient.list",
            &[Some("env:API_URL"), Some("host:localhost:7007")],
        ),
        ("LegacyClient.list", &[None]),
    ];
    for &(function, expected) in expected {
        let actual = vias.get_mut(function).unwrap();
        actual.sort_unstable();
        assert_eq!(actual, expected, "{function}");
    }

    let links: Vec<(&str, &str, &str)> = export["links"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| {
            let consumer = l["consumer"].as_str().unwrap();
            [
                "viaGetString",
                "OrdersClient",
                "ParameterClient",
                "AssignedClient",
                "viaIncludedKey",
                "viaIncludedBlock",
            ]
            .iter()
            .any(|f| consumer.contains(f))
        })
        .map(|l| {
            (
                l["provider"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
                l["reason"].as_str().unwrap(),
            )
        })
        .collect();
    let link = (
        "example.com/backstage/api::src/server.ts::GET /orders",
        "inferred",
        "base URL from API_URL, a base_url_env of peer example.com/backstage/api",
    );
    assert_eq!(links, [link; 6]);
}

#[test]
fn context_names_the_base_url_and_selected_peer_after_a_partial_edit_and_restore() {
    let case = "xlang_backstage_context_reason";
    let web = common::fixture_repo(
        case,
        "ts-backstage-web",
        Some("https://example.com/backstage/web.git"),
    );
    for (fixture, origin) in [
        ("ts-backstage-api", "https://example.com/backstage/api.git"),
        (
            "ts-backstage-other",
            "https://example.com/backstage/other.git",
        ),
    ] {
        common::fixture_repo(case, fixture, Some(origin));
    }
    let config = web.join("meada.toml");
    fs::write(
        &config,
        fs::read_to_string(&config).unwrap() + "base_url_env = [\"ALT_URL\"]\n",
    )
    .unwrap();
    fs::write(
        web.join("src/partial-client.ts"),
        "import { Config } from '@backstage/config';\n\
         export class PartialClient {\n\
           private readonly baseUrl: string;\n\
           constructor(config: Config) {\n\
             this.baseUrl = config.getString('included.keyed.baseUrl');\n\
           }\n\
           orders() {\n\
             return fetch(`${this.baseUrl}/orders`);\n\
           }\n\
         }\n",
    )
    .unwrap();
    common::age(&web);
    let engine = Engine::open(&web).unwrap();
    let control = engine.context("PublicClient.list", None).unwrap();
    assert_eq!(
        control
            .lines()
            .filter(|line| line.contains(" consumes "))
            .collect::<Vec<_>>(),
        ["example.com/backstage/web src/clients.ts:25 consumes (unresolved) fetch"]
    );
    let partial = web.join("config/partials/acme.yaml");
    let original = fs::read_to_string(&partial).unwrap();
    let mut answers = Vec::new();
    for (variable, peer) in [("API_URL", "api"), ("ALT_URL", "other"), ("API_URL", "api")] {
        fs::write(&partial, original.replace("API_URL", variable)).unwrap();
        let context = engine.context("PartialClient.orders", None).unwrap();
        let trace = engine
            .trace(Start::From("PartialClient.orders"), Direction::Down, None)
            .unwrap();
        let reason = format!(
            "base URL from {variable}, a base_url_env of peer example.com/backstage/{peer}"
        );
        let provider = format!("example.com/backstage/{peer}::src/server.ts::GET /orders");
        let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
        let site =
            "example.com/backstage/web::src/partial-client.ts::PartialClient.orders::fetch#0";
        let links: Vec<_> = export["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|link| link["consumer"] == site)
            .map(|link| {
                (
                    link["contract"].as_str().unwrap(),
                    link["provider"].as_str().unwrap(),
                    link["level"].as_str().unwrap(),
                    link["reason"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            links,
            [(
                "http::GET::/orders",
                provider.as_str(),
                "inferred",
                reason.as_str()
            )]
        );
        let consumer = export["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|consumer| consumer["site"] == site)
            .unwrap();
        assert_eq!(consumer["via"], format!("env:{variable}"));
        assert!(trace.contains(&format!("→[inferred: {reason}]")), "{trace}");
        let consumed: Vec<_> = context
            .lines()
            .filter(|line| line.contains(" consumes "))
            .collect();
        assert_eq!(
            consumed,
            [format!(
                "example.com/backstage/web src/partial-client.ts:8 consumes http::GET::/orders →[inferred: {reason}] {provider}"
            )],
            "{context}"
        );
        assert_golden(&format!("backstage.context-partial-{peer}.txt"), &context);
        assert_eq!(engine.context("PublicClient.list", None).unwrap(), control);
        answers.push((context, trace));
    }
    assert_ne!(answers[0], answers[1]);
    assert_eq!(answers[0], answers[2]);
}

#[test]
fn context_keeps_non_environment_inferred_consumers_plain() {
    let case = "xlang_backstage_context_controls";
    let web = common::fixture_repo(
        case,
        "ts-backstage-web",
        Some("https://example.com/backstage/web.git"),
    );
    let api = common::fixture_repo(
        case,
        "ts-backstage-api",
        Some("https://example.com/backstage/api.git"),
    );
    fs::write(
        web.join("meada.toml"),
        "[[peer]]\nrepo = 'example.com/backstage/api'\nbase_url_env = ['API_URL']\n",
    )
    .unwrap();
    fs::write(
        api.join("meada.toml"),
        "[serve]\nhosts = ['api.example.com']\nprefix = '/gateway'\n",
    )
    .unwrap();
    fs::write(
        web.join("src/controls.ts"),
        "export function ordersTpl(base: string) { return fetch(`${base}/orders`); }\n\
         export function ordersRelative() { return fetch('/orders'); }\n\
         export function ordersHost() { return fetch('https://api.example.com/gateway/orders'); }\n",
    )
    .unwrap();
    let engine = Engine::open(&web).unwrap();
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let tpl = "example.com/backstage/web::src/controls.ts::ordersTpl::fetch#0";
    let links = export["links"].as_array().unwrap();
    assert!(
        links.iter().all(|link| link["consumer"] != tpl),
        "{links:?}"
    );
    let items = engine.open_items().unwrap();
    let item = items.iter().find(|item| item.id == tpl).unwrap();
    assert_eq!(
        item.limit.as_ref().map(|l| l.describe()).as_deref(),
        Some("base from the parameter base")
    );
    let mut consumed = Vec::new();
    for (method, path, via, reason) in [
        (
            "ordersRelative",
            "/orders",
            None,
            "host unresolved; the only repository serving it is example.com/backstage/api",
        ),
        (
            "ordersHost",
            "/gateway/orders",
            Some("host:api.example.com"),
            "gateway prefix /gateway",
        ),
    ] {
        let site = format!("example.com/backstage/web::src/controls.ts::{method}::fetch#0");
        let consumer = export["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|consumer| consumer["site"] == site)
            .unwrap();
        assert_eq!(consumer["contract"], format!("http::GET::{path}"));
        assert_eq!(consumer["via"].as_str(), via);
        let links: Vec<_> = export["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|link| link["consumer"] == site)
            .map(|link| {
                (
                    link["provider"].as_str().unwrap(),
                    link["level"].as_str().unwrap(),
                    link["reason"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            links,
            [(
                "example.com/backstage/api::src/server.ts::GET /orders",
                "inferred",
                reason
            )]
        );
        let trace = engine
            .trace(Start::From(method), Direction::Down, None)
            .unwrap();
        assert!(trace.contains(&format!("→[inferred: {reason}]")), "{trace}");
        let context = engine.context(method, None).unwrap();
        consumed.extend(
            context
                .lines()
                .filter(|line| line.contains(" consumes "))
                .map(str::to_string),
        );
    }
    assert_eq!(
        consumed,
        [
            "example.com/backstage/web src/controls.ts:2 consumes http::GET::/orders",
            "example.com/backstage/web src/controls.ts:3 consumes http::GET::/gateway/orders",
        ]
    );
}

#[test]
fn context_explains_only_a_base_url_that_selects_a_declared_peer() {
    let case = "xlang_backstage_context_unmapped";
    let web = common::fixture_repo(
        case,
        "ts-backstage-web",
        Some("https://example.com/backstage/web.git"),
    );
    common::fixture_repo(
        case,
        "ts-backstage-api",
        Some("https://example.com/backstage/api.git"),
    );
    fs::write(
        web.join("src/unmapped.ts"),
        "export function ordersUnknownEnv() { return fetch(`${process.env.UNKNOWN_URL}/orders`); }\n\
         export function ordersProxy() { return fetch('/api/proxy/unknown/orders'); }\n",
    )
    .unwrap();
    fs::write(
        web.join("app-config.yaml"),
        "proxy:\n  endpoints:\n    '/unknown':\n      target: ${UNKNOWN_URL}\n",
    )
    .unwrap();
    let config = web.join("meada.toml");
    let provider = "example.com/backstage/api::src/server.ts::GET /orders";
    let mut answers = Vec::new();
    for mapped in [false, true, false] {
        let peer = "[[peer]]\nrepo = 'example.com/backstage/api'\n";
        let mapping = if mapped {
            "base_url_env = ['UNKNOWN_URL']\n"
        } else {
            ""
        };
        fs::write(&config, format!("{peer}{mapping}")).unwrap();
        let engine = Engine::open(&web).unwrap();
        let mut consumed = Vec::new();
        let mut expected = Vec::new();
        let mut contexts = Vec::new();
        for (method, line, through) in [
            ("ordersUnknownEnv", 1, ""),
            ("ordersProxy", 2, " through the proxy endpoint /unknown"),
        ] {
            let context = engine.context(method, None).unwrap();
            let trace = engine
                .trace(Start::From(method), Direction::Down, None)
                .unwrap();
            let reason = match mapped {
                true => format!(
                    "base URL from UNKNOWN_URL{through}, a base_url_env of peer example.com/backstage/api"
                ),
                false => format!(
                    "base URL from UNKNOWN_URL{through}, no peer's base_url_env; the only repository serving it is example.com/backstage/api"
                ),
            };
            assert!(trace.contains(&format!("→[inferred: {reason}]")), "{trace}");
            let export: serde_json::Value =
                serde_json::from_str(&engine.export().unwrap()).unwrap();
            let site = format!("example.com/backstage/web::src/unmapped.ts::{method}::fetch#0");
            let links: Vec<_> = export["links"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|link| link["consumer"] == site)
                .map(|link| {
                    (
                        link["contract"].as_str().unwrap(),
                        link["provider"].as_str().unwrap(),
                        link["level"].as_str().unwrap(),
                        link["reason"].as_str().unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                links,
                [("http::GET::/orders", provider, "inferred", reason.as_str())]
            );
            consumed.extend(
                context
                    .lines()
                    .filter(|line| line.contains(" consumes "))
                    .map(str::to_string),
            );
            let plain = format!(
                "example.com/backstage/web src/unmapped.ts:{line} consumes http::GET::/orders"
            );
            expected.push(match mapped {
                true => format!("{plain} →[inferred: {reason}] {provider}"),
                false => plain,
            });
            contexts.push((context, trace));
        }
        assert_eq!(consumed, expected);
        answers.push(contexts);
    }
    assert_ne!(answers[0], answers[1]);
    assert_eq!(answers[0], answers[2]);
}

#[test]
fn a_pydantic_settings_field_is_the_variable_it_reads() {
    let case = "xlang_py_settings";
    let web = common::fixture_repo(
        case,
        "py-settings-web",
        Some("https://example.com/pysettings/web.git"),
    );
    for (fixture, origin) in [
        ("py-settings-api", "https://example.com/pysettings/api.git"),
        (
            "py-settings-other",
            "https://example.com/pysettings/other.git",
        ),
    ] {
        common::fixture_repo(case, fixture, Some(origin));
    }
    let export: serde_json::Value =
        serde_json::from_str(&Engine::open(&web).unwrap().export().unwrap()).unwrap();
    let links = |function: &str| -> Vec<(String, String, String)> {
        (export["links"].as_array().unwrap().iter())
            .filter(|l| {
                l["consumer"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("::{function}"))
                    || l["consumer"]
                        .as_str()
                        .unwrap()
                        .contains(&format!("::{function}::"))
            })
            .map(|l| {
                ["provider", "level", "reason"]
                    .map(|k| l[k].as_str().unwrap().to_string())
                    .into()
            })
            .collect()
    };
    for (function, variable) in [
        ("via_prefix", "APP_API_URL"),
        ("via_alias", "ORDERS_URL"),
        ("no_default", "APP_REQUIRED"),
        ("validation_alias", "BOTH_URL"),
    ] {
        let found = links(function);
        assert_eq!(found.len(), 1, "{function}: {found:?}");
        assert_eq!(found[0].0, "example.com/pysettings/api::server.py::orders");
        assert_eq!(found[0].1, "inferred");
        assert_eq!(
            found[0].2,
            format!("base URL from {variable}, a base_url_env of peer example.com/pysettings/api")
        );
    }
    let plain = [links("not_settings"), links("class_var")].concat();
    assert!(
        !plain.is_empty() && plain.iter().all(|l| l.1 == "ambiguous"),
        "{plain:?}"
    );
}
