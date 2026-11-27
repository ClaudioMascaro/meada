//! Open items (spec #18): the ambiguous links and unresolved consumers of a workspace, their
//! sorted candidates, and how `search` lists them.

use crate::common;

use std::fs;

use common::{age, assert_golden, chain, fixture_repo, meada};
use meada::engine::Engine;

fn few(case: &str) -> Engine {
    let repo = fixture_repo(case, "open-few", Some("https://example.com/open/few.git"));
    Engine::open(&repo).unwrap()
}

fn generated(case: &str) -> Engine {
    let origin = Some("https://example.com/open/generated.git");
    Engine::open(&fixture_repo(case, "open-generated", origin)).unwrap()
}

#[test]
fn search_lists_the_ambiguous_links_of_the_chain_with_a_decide_hint() {
    let (web, _, _) = chain("items_ambiguous");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "chain.search-ambiguous.txt",
        &engine.search("", Some("ambiguous"), None).unwrap(),
    );
}

#[test]
fn search_lists_the_unresolved_consumers_of_the_chain() {
    let (web, _, _) = chain("items_unresolved");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "chain.search-unresolved.txt",
        &engine.search("", Some("unresolved"), None).unwrap(),
    );
}

#[test]
fn search_matches_the_enclosing_symbol_and_the_call_expression() {
    let (web, _, _) = chain("items_query");
    let engine = Engine::open(&web).unwrap();

    // The enclosing symbol is `track`; the call expression holds `event.url`.
    for query in ["track", "event.url"] {
        let answer = engine.search(query, Some("unresolved"), None).unwrap();
        assert!(answer.contains("src/status.ts:8"), "{query}: {answer}");
    }
    assert_eq!(
        engine.search("track", Some("ambiguous"), None).unwrap(),
        "no matches for `track`\n"
    );
}

#[test]
fn a_dynamic_url_lists_the_routes_its_method_and_fragments_leave() {
    let engine = few("items_few");

    let items = engine.open_items().unwrap();

    assert_eq!(items.len(), 1);
    let routes: Vec<String> = (items[0].candidates.iter())
        .map(|c| format!("{} {}", c.repository, c.label()))
        .collect();
    // The DELETE and POST routes, `/api/users` and `/api/health` are filtered out.
    assert_eq!(
        routes,
        [
            "example.com/open/few GET /api/reports/summary",
            "example.com/open/few GET /api/reports/{}",
            "example.com/open/few GET /api/reports/{}/download",
        ]
    );
    assert_golden(
        "open-few.search-unresolved.txt",
        &engine.search("report", Some("unresolved"), None).unwrap(),
    );
}

#[test]
fn generated_routes_leave_a_dynamic_url_more_than_fifty_candidates() {
    let engine = generated("items_generated");

    let items = engine.open_items().unwrap();

    assert_eq!(items[0].candidates.len(), 55);
    assert_golden(
        "open-generated.search-unresolved.txt",
        &engine.search("", Some("unresolved"), None).unwrap(),
    );
}

#[test]
fn ambiguous_candidates_are_the_listed_providers_sorted_by_repository() {
    let (web, _, _) = chain("items_order");
    let items = Engine::open(&web).unwrap().open_items().unwrap();

    // Item ids are call-site identities, handled in id order.
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "example.com/chain/web::src/status.ts::health::fetch#0",
            "example.com/chain/web::src/status.ts::track::fetch#0",
        ]
    );
    let providers: Vec<&str> = (items[0].candidates.iter())
        .map(|c| c.site.as_str())
        .collect();
    assert_eq!(
        providers,
        [
            "example.com/chain/catalog::app/api/health/route.ts::GET",
            "example.com/chain/gateway::src/server.ts::GET /api/health",
        ]
    );
}

#[test]
fn a_refreshed_index_lists_the_open_items_a_cold_index_does() {
    // The method and fragments of an unresolved consumer are not in the export, so this is
    // where a refresh that loses them shows.
    let render = |engine: &Engine| {
        let mut out = engine.search("", Some("unresolved"), None).unwrap();
        for item in engine.open_items().unwrap() {
            out += &format!("{} {:?}\n", item.id, item.candidates);
        }
        out
    };
    let edit = |repo: &std::path::Path| {
        let latest = repo.join("app/api/reports/latest");
        fs::create_dir_all(&latest).unwrap();
        fs::write(
            latest.join("route.ts"),
            "export function GET() {\n  return new Response(\"latest\");\n}\n",
        )
        .unwrap();
        fs::write(repo.join("app/api/users/route.ts"), "export const x = 1;\n").unwrap();
    };
    let origin = Some("https://example.com/open/few.git");
    let repo = fixture_repo("items_fresh", "open-few", origin);
    age(&repo);
    let before = render(&Engine::open(&repo).unwrap());
    edit(&repo);

    let refreshed = render(&Engine::open(&repo).unwrap());

    let cold = fixture_repo("items_fresh_cold", "open-few", origin);
    edit(&cold);
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, render(&Engine::open(&cold).unwrap()));
}

/// The issue's repositories (#801): `api` serves `GET /api/orders`, `/orders`, `/v2/orders`,
/// `/items`, `/api/orders/items`, `/api/items/export` and `/api/lineitems`; `go` calls it with
/// bases read in place and with one a method returns. `client` is the client's fixture.
fn unread_base(case: &str, client: &str) -> std::path::PathBuf {
    fixture_repo(
        case,
        "unread-base-api",
        Some("https://example.com/acme/api.git"),
    );
    let origin = format!(
        "https://example.com/acme/{}.git",
        &client["unread-base-".len()..]
    );
    fixture_repo(case, client, Some(&origin))
}

/// Every link of `engine`'s export as `consumer -> contract [reason]`.
fn links_of(engine: &Engine) -> Vec<String> {
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    (export["links"].as_array().unwrap().iter())
        .map(|l| {
            let at = l["consumer"].as_str().unwrap();
            let call = at.split_once("::").map_or(at, |(_, call)| call);
            format!(
                "{call} -> {} [{}]",
                l["contract"].as_str().unwrap(),
                l["reason"].as_str().unwrap()
            )
        })
        .collect()
}

/// Each open item as its call, its limit and its candidates' labels.
fn items_of(engine: &Engine) -> Vec<String> {
    (engine.open_items().unwrap().iter())
        .map(|i| {
            let routes: Vec<String> = i.candidates.iter().map(|c| c.label()).collect();
            let call =
                i.id.split_once("::")
                    .map_or(i.id.as_str(), |(_, call)| call);
            format!("{call}: {}", routes.join(", "))
        })
        .collect()
}

/// A URL whose base is the return value of a call Meada does not evaluate (`c.baseV2()`, which
/// adds `/v2`) links to no route: it is an open item whose candidates are the routes that end
/// with its known part, and it names the limit and the callee. A base read in place still links
/// by its variable, whether a peer names it or not.
#[test]
fn a_base_a_call_returns_makes_an_open_item_that_names_the_callee() {
    let go = unread_base("items_unread_base_call", "unread-base-go");
    let engine = Engine::open(&go).unwrap();

    assert_eq!(
        items_of(&engine),
        [
            "client.go::Client.ViaMethodPrefix::http.Get#0: GET /api/orders, GET /orders, GET /v2/orders"
        ]
    );
    assert_eq!(
        links_of(&engine),
        [
            "client.go::Client.Inline::http.Get#0 -> http::GET::/api/orders [base URL from \
             API_URL, a base_url_env of peer example.com/acme/api]",
            "client.go::Client.InlineOther::http.Get#0 -> http::GET::/v2/orders [base URL from \
             OTHER_URL, no peer's base_url_env; the only repository serving it is \
             example.com/acme/api]",
        ]
    );
    let search = engine.search("", Some("unresolved"), None).unwrap();
    assert!(
        search.contains("candidates: 3; limit: base from the return value of c.baseV2;"),
        "{search}"
    );
    let (listed, _, ok) = meada(&go, &["decide", "--list"], "", |c| {
        c.env("LANG", "en_US.UTF-8");
    });
    assert!(ok);
    assert!(
        listed.contains(
            "client.go:16  http.Get(c.baseV2() + \"/orders\")\n  \
             kind = \"dynamic-url\"\n  \
             limit: base from the return value of c.baseV2\n  1) "
        ),
        "{listed}"
    );
}

/// A base that is a parameter no call site binds, or the return value of a method, makes an open
/// item whose candidates end with the known part at a `/`: `/items` takes `GET /items` and `GET
/// /api/orders/items`, not `GET /api/items/export` nor `GET /api/lineitems`. A URL written with
/// no base, and an absolute path resolved over an unknown base, keep the hostless link.
#[test]
fn a_base_a_parameter_holds_makes_an_open_item_and_a_certain_path_still_links() {
    let web = unread_base("items_unread_base_param", "unread-base-web");
    let engine = Engine::open(&web).unwrap();

    assert_eq!(
        items_of(&engine),
        [
            "src/client.ts::OrdersClient.list::fetch#0: GET /api/orders, GET /orders, GET /v2/orders",
            "src/client.ts::byParam::fetch#0: GET /api/orders/items, GET /items",
        ]
    );
    let search = engine.search("", Some("unresolved"), None).unwrap();
    for limit in [
        "limit: base from the parameter base;",
        "limit: base from the return value of this.base;",
    ] {
        assert!(search.contains(limit), "{limit}: {search}");
    }
    let hostless = "[host unresolved; the only repository serving it is example.com/acme/api]";
    assert_eq!(
        links_of(&engine),
        [
            format!("src/client.ts::plain::fetch#0 -> http::GET::/orders {hostless}"),
            format!("src/client.ts::viaUrl::fetch#0 -> http::GET::/orders {hostless}"),
        ]
    );
}

/// A `dynamic-url` decision that names one of the item's candidates applies on the next index.
#[test]
fn a_decision_for_a_candidate_of_an_unread_base_applies() {
    let go = unread_base("items_unread_base_decided", "unread-base-go");
    common::decisions(&go, "unread-base-go");
    let engine = Engine::open(&go).unwrap();

    assert_eq!(items_of(&engine), Vec::<String>::new());
    assert!(
        links_of(&engine).contains(
            &"client.go::Client.ViaMethodPrefix::http.Get#0 -> http::GET::/v2/orders [decided by human]"
                .to_string()
        ),
        "{:?}",
        links_of(&engine)
    );
}

/// An `ambiguous-link` decision written for such a call goes stale, and the item stays open.
#[test]
fn an_ambiguous_link_decision_for_an_unread_base_goes_stale() {
    let go = unread_base("items_unread_base_stale", "unread-base-go");
    common::decisions(&go, "unread-base-go-ambiguous");
    let engine = Engine::open(&go).unwrap();

    assert_eq!(
        items_of(&engine),
        [
            "client.go::Client.ViaMethodPrefix::http.Get#0: GET /api/orders, GET /orders, GET /v2/orders"
        ]
    );
    let (listed, _, ok) = meada(&go, &["decide", "--list"], "", |c| {
        c.env("LANG", "en_US.UTF-8");
    });
    assert!(ok);
    assert!(
        listed.contains(
            "stale decision: example.com/acme/go::client.go::Client.ViaMethodPrefix::http.Get#0 \
             -> example.com/acme/api::http::GET::/v2/orders (kind `ambiguous-link` needs to be \
             `dynamic-url`. Use kind = \"dynamic-url\")"
        ),
        "{listed}"
    );
}

/// Each open item as its call and the limit it names.
fn limits_of(engine: &Engine) -> Vec<String> {
    (engine.open_items().unwrap().iter())
        .map(|i| {
            let call =
                i.id.split_once("::")
                    .map_or(i.id.as_str(), |(_, call)| call);
            let limit = i.limit.as_ref().map(|l| l.describe());
            format!("{call}: {}", limit.as_deref().unwrap_or("none"))
        })
        .collect()
}

/// A Python `httpx` client built with a `base_url` Meada does not read (it comes from another
/// module), and a URL that starts with a call's return value, are open items that name the
/// client's base and the callee; neither links to a guessed route. An aiohttp session joins as
/// RFC 3986 does: over such a base, a relative path is an open item and a path from the root
/// keeps the hostless link.
#[test]
fn a_python_base_meada_did_not_read_makes_open_items_that_name_it() {
    let python = unread_base("items_unread_base_python", "unread-base-python");
    let engine = Engine::open(&python).unwrap();

    assert_eq!(
        items_of(&engine),
        [
            "client.py::via_call::requests.get#0: GET /api/orders/items, GET /items",
            "client.py::via_client::client.get#0: GET /api/orders, GET /orders, GET /v2/orders",
            "client.py::via_session::session.get#1: GET /api/orders/items, GET /items",
        ]
    );
    assert_eq!(
        limits_of(&engine),
        [
            "client.py::via_call::requests.get#0: base from the return value of load_base",
            "client.py::via_client::client.get#0: base of a client Meada did not read",
            "client.py::via_session::session.get#1: base of a client Meada did not read",
        ]
    );
    assert_eq!(
        links_of(&engine),
        [
            "client.py::via_session::session.get#0 -> http::GET::/orders [host unresolved; the only \
          repository serving it is example.com/acme/api]"
        ]
    );
}

/// Swift: a URL that starts with a call's return value, and a path component appended to a URL
/// Meada did not read, are open items that name the callee and the client's base; a path from
/// the root resolved over such a URL (`URL(string:relativeTo:)`) keeps the hostless link.
#[test]
fn a_swift_base_meada_did_not_read_makes_open_items_that_name_it() {
    let swift = unread_base("items_unread_base_swift", "unread-base-swift");
    let engine = Engine::open(&swift).unwrap();

    let at = "Sources/Web/Client.swift";
    assert_eq!(
        items_of(&engine),
        [
            format!(
                "{at}::viaCall()::URLSession.shared.data#0: GET /api/orders, GET /orders, GET /v2/orders"
            ),
            format!(
                "{at}::viaComponent()::URLSession.shared.data#0: GET /api/orders/items, GET /items"
            ),
        ]
    );
    assert_eq!(
        limits_of(&engine),
        [
            format!(
                "{at}::viaCall()::URLSession.shared.data#0: base from the return value of loadBase"
            ),
            format!(
                "{at}::viaComponent()::URLSession.shared.data#0: base of a client Meada did not read"
            ),
        ]
    );
    assert_eq!(
        links_of(&engine),
        [format!(
            "{at}::viaRelative()::URLSession.shared.data#0 -> http::GET::/orders [host \
             unresolved; the only repository serving it is example.com/acme/api]"
        )]
    );
}

/// The key of a Go struct literal named like an import (`&Client{http: ..}`, `&Client{os: ..}`)
/// is a field: the file still reads the package, so each call keeps its method and its base,
/// as with another key (`good`) or none (`nolit`), and links with no open item.
#[test]
fn a_go_literal_key_named_like_an_import_leaves_its_call_linked() {
    fixture_repo(
        "items_go_literal_key",
        "go-literal-key-api",
        Some("https://example.com/acme/api.git"),
    );
    let cli = fixture_repo(
        "items_go_literal_key",
        "go-literal-key-cli",
        Some("https://example.com/acme/cli.git"),
    );
    let engine = Engine::open(&cli).unwrap();

    assert_eq!(items_of(&engine), Vec::<String>::new());
    let reason = "base URL from API_URL, a base_url_env of peer example.com/acme/api";
    assert_eq!(
        links_of(&engine),
        ["bad", "good", "nolit", "osk"].map(|package| format!(
            "{package}/x.go::Client.CreateOrder::http.NewRequestWithContext#0 -> \
             http::POST::/orders [{reason}]"
        ))
    );
    let export: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let levels = (export["links"].as_array().unwrap().iter()).map(|l| l["level"].clone());
    assert_eq!(levels.collect::<Vec<_>>(), ["inferred"; 4]);
}

fn http_value_flow(case: &str) -> serde_json::Value {
    fixture_repo(
        case,
        "http-value-flow-api",
        Some("https://example.com/flow/api.git"),
    );
    let client = fixture_repo(
        case,
        "http-value-flow",
        Some("https://example.com/flow/client.git"),
    );
    serde_json::from_str(&Engine::open(&client).unwrap().export().unwrap()).unwrap()
}

#[test]
fn exact_returns_keys_and_optional_queries_reach_http_consumers() {
    let export = http_value_flow("http_value_flow_folded");
    let consumers = export["consumers"].as_array().unwrap();
    let links = export["links"].as_array().unwrap();
    let mut differences = Vec::new();
    for path in [
        "client.go",
        "client.py",
        "client.ts",
        "Client.java",
        "Client.kt",
        "Client.cs",
        "Sources/Client/Client.swift",
        "src/lib.rs",
    ] {
        let cases = [
            ("ViaMethod", "/api/orders", "API_URL"),
            ("ViaFunction", "/v2/orders", "API_URL"),
            ("MethodWithPrefix", "/v2/orders", "API_URL"),
            ("FunctionWithoutPrefix", "/api/orders", "API_URL"),
            ("ParamA", "/api/orders", "API_URL"),
            ("ParamB", "/api/orders", "SECOND_URL"),
            ("QueryAppend", "/api/orders", "API_URL"),
            ("QueryRewrite", "/api/orders", "API_URL"),
            ("QuerySuffix", "/api/orders", "API_URL"),
            ("QueryConditional", "/api/orders", "API_URL"),
        ];
        for (name, route, variable) in cases {
            if path == "client.go" && name == "QueryConditional" {
                continue;
            }
            let matches: Vec<_> = consumers
                .iter()
                .filter(|c| {
                    let site = c["site"].as_str().unwrap();
                    site.contains(&format!("::{path}::")) && site.contains(name)
                })
                .collect();
            let contract = format!("http::GET::{route}");
            let via = format!("env:{variable}");
            let found = match matches.as_slice() {
                [row] if row["contract"] == contract && row["via"] == via => {
                    let linked: Vec<_> = links
                        .iter()
                        .filter(|l| l["consumer"] == row["site"])
                        .collect();
                    matches!(linked.as_slice(), [link] if link["contract"] == contract
                        && link["level"] == "inferred"
                        && link["provider"].as_str().unwrap().starts_with("example.com/flow/api::openapi.yaml::"))
                }
                _ => false,
            };
            if !found {
                differences.push(format!("{path} {name}: {matches:?}"));
            }
        }
        let unknown: Vec<_> = consumers
            .iter()
            .filter(|c| {
                let site = c["site"].as_str().unwrap();
                site.contains(&format!("::{path}::")) && site.contains("ParamUnknown")
            })
            .collect();
        if !matches!(unknown.as_slice(), [row] if row["contract"].is_null() && row["via"].is_null())
        {
            differences.push(format!("{path} ParamUnknown: {unknown:?}"));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn an_awaited_helper_and_discovery_arguments_keep_their_base() {
    let export = http_value_flow("http_value_flow_async");
    let consumers = export["consumers"].as_array().unwrap();
    for (name, route, via) in [
        ("ViaAwait", "http::GET::/v2/orders", "env:API_URL"),
        (
            "PluginMethod",
            "http::GET::/api/orders/items",
            "client:Backstage plugin orders at /api/orders",
        ),
        (
            "PluginParameter",
            "http::GET::/api/orders/items",
            "client:Backstage plugin orders at /api/orders",
        ),
    ] {
        let rows: Vec<_> = consumers
            .iter()
            .filter(|c| c["site"].as_str().unwrap().contains(name))
            .collect();
        assert!(
            matches!(rows.as_slice(), [row] if row["contract"] == route && row["via"] == via),
            "{name}: {rows:?}"
        );
    }
}

#[test]
fn unknown_returns_and_path_changes_remain_unresolved() {
    let export = http_value_flow("http_value_flow_unknown");
    let consumers = export["consumers"].as_array().unwrap();
    for name in [
        "ViaPromise",
        "ViaDispatch",
        "ViaRecursive",
        "ViaUnknownReturn",
        "ViaNameReturn",
        "ViaDivergent",
        "unsafeDispatch",
        "PathChanged",
        "PathReplaced",
        "MiddleQuery",
        "BrowserQueryOnly",
    ] {
        let rows: Vec<_> = consumers
            .iter()
            .filter(|c| c["site"].as_str().unwrap().contains(name))
            .collect();
        assert!(
            matches!(rows.as_slice(), [row] if row["contract"].is_null()),
            "{name}: {rows:?}"
        );
    }
    let unknown: Vec<_> = consumers
        .iter()
        .filter(|c| c["site"].as_str().unwrap().contains("PathUnknown"))
        .collect();
    assert!(matches!(unknown.as_slice(), [row] if row["contract"] == "http::GET::/api/{}"));
    assert!(!consumers.iter().any(|c| {
        c["site"].as_str().unwrap().contains("ViaNameMatch") && !c["contract"].is_null()
    }));
}

#[test]
fn a_go_deferred_write_of_the_named_result_leaves_the_base_open() {
    // #948: the deferred function writes `base` after `return` set it; the empty one does not.
    let export = http_value_flow("http_value_flow_deferred");
    let consumers = export["consumers"].as_array().unwrap();
    let links = export["links"].as_array().unwrap();
    let rows = |name: &str| -> Vec<&serde_json::Value> {
        let at = format!("::client.go::{name}::");
        (consumers.iter())
            .filter(|c| c["site"].as_str().unwrap().contains(&at))
            .collect()
    };
    let deferred = rows("ViaDeferredBase");
    assert!(
        matches!(deferred.as_slice(), [row] if row["contract"].is_null() && row["via"].is_null()
            && !links.iter().any(|l| l["consumer"] == row["site"])),
        "{deferred:?}"
    );
    let benign = rows("ViaBenignDefer");
    assert!(
        matches!(benign.as_slice(), [row] if row["contract"] == "http::GET::/api/orders"
            && row["via"] == "env:API_URL"),
        "{benign:?}"
    );
}

#[test]
fn an_environment_key_needs_a_reader_bound_to_the_runtime() {
    let export = http_value_flow("http_value_flow_env_reader");
    let consumers = export["consumers"].as_array().unwrap();
    let links = export["links"].as_array().unwrap();
    let shadows: Vec<_> = consumers
        .iter()
        .filter(|row| {
            let site = row["site"].as_str().unwrap();
            site.contains("ShadowedEnvKey")
                || site.contains("shadowed_read")
                || site.contains("shadowedRead")
        })
        .collect();
    assert!(
        shadows
            .iter()
            .any(|row| row["site"].as_str().unwrap().contains("client.py")),
        "{shadows:?}"
    );
    assert!(
        shadows
            .iter()
            .any(|row| row["site"].as_str().unwrap().contains("client.ts")),
        "{shadows:?}"
    );
    for row in shadows {
        assert_ne!(row["via"], "env:API_URL", "{row:?}");
        assert!(!links.iter().any(|link| link["consumer"] == row["site"]
            && link["reason"].as_str().unwrap().contains("base_url_env")));
    }
    let imported = consumers
        .iter()
        .find(|row| row["site"].as_str().unwrap().contains("ImportedEnvKey"))
        .unwrap();
    assert_eq!(imported["via"], "env:API_URL", "{imported:?}");
    assert!(
        links
            .iter()
            .any(|link| link["consumer"] == imported["site"] && link["level"] == "inferred")
    );
}

#[test]
fn a_reassigned_parameter_does_not_take_the_callers_value() {
    let export = http_value_flow("http_value_flow_reassigned");
    for path in ["client.py", "client.ts"] {
        let consumers: Vec<_> = export["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                row["site"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("::{path}::"))
            })
            .collect();
        for name in [
            "mutated",
            if path.ends_with(".ts") {
                "possiblyMutated"
            } else {
                "possibly_mutated"
            },
        ] {
            let rows: Vec<_> = consumers
                .iter()
                .filter(|c| c["site"].as_str().unwrap().contains(&format!("::{name}::")))
                .collect();
            assert!(
                matches!(rows.as_slice(), [row] if row["contract"].is_null()),
                "{path} {name}: {rows:?}"
            );
        }
        let intact: Vec<_> = consumers
            .iter()
            .filter(|c| c["site"].as_str().unwrap().contains("::Intact::"))
            .collect();
        assert!(matches!(intact.as_slice(), [row] if row["contract"] == "http::GET::/api/orders"));
        for name in ["::Reassigned::", "::Conditional::"] {
            assert!(!consumers.iter().any(|c| c["site"].as_str().unwrap().contains(name) && !c["contract"].is_null()));
        }
    }
    let writes: Vec<_> = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["site"]
                .as_str()
                .unwrap()
                .contains("::mutation/store.go::")
                && !row["contract"].is_null()
        })
        .map(|row| {
            (
                row["site"].as_str().unwrap(),
                row["contract"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        writes,
        [
            (
                "example.com/flow/client::mutation/store.go::write::client.PutItem#0",
                "resource::dynamodb"
            ),
            (
                "example.com/flow/client::mutation/store.go::writeConditional::client.PutItem#0",
                "resource::dynamodb"
            ),
            (
                "example.com/flow/client::mutation/store.go::Current::writeIntact#0",
                "resource::dynamodb::orders"
            ),
        ]
    );
    let cycle: Vec<_> = export["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["site"].as_str().unwrap().contains("::ViaCycle::"))
        .collect();
    assert!(
        cycle
            .iter()
            .any(|r| r["contract"] == "http::GET::/api/orders")
    );
    assert!(
        export["consumers"].as_array().unwrap().iter().any(|r| {
            r["site"].as_str().unwrap().contains("::cycle::") && r["contract"].is_null()
        })
    );
    assert!(
        !cycle
            .iter()
            .any(|r| r["contract"] == "http::GET::/v2/orders")
    );
}

#[test]
fn a_rotating_go_wrapper_leaves_its_neighbors_readable() {
    let since = std::time::Instant::now();
    let export = http_value_flow("http_value_flow_cycle");
    let consumers: Vec<_> = export["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["site"]
                .as_str()
                .unwrap()
                .contains("::recursive/recursive.go::")
        })
        .collect();
    for (name, contract) in [
        ("PlainA", "http::GET::/plain-a"),
        ("PlainB", "http::GET::/plain-b"),
        ("UseGet", "http::GET::/g0"),
    ] {
        assert!(
            consumers
                .iter()
                .any(|row| row["site"].as_str().unwrap().contains(name)
                    && row["contract"] == contract),
            "{name}: {consumers:?}"
        );
    }
    assert!(since.elapsed() < std::time::Duration::from_secs(20));
}

#[test]
fn exported_open_items_supply_canonical_decisions_and_keep_the_full_candidate_set() {
    let (web, _, _) = chain("items_export_contract");
    let read = || Engine::open(&web).unwrap().export().unwrap();
    let first = read();
    assert_eq!(read(), first);
    let parsed = meada_core::export::parse(&first).unwrap();
    assert_eq!(parsed.open_items.len(), 2);
    let decoded: serde_json::Value = serde_json::from_str(&first).unwrap();
    let data = decoded["open_items"].as_array().unwrap();
    assert_eq!(data.len(), 2, "{data:?}");
    assert_eq!(data[0]["kind"], "ambiguous-link");
    assert_eq!(data[1]["kind"], "dynamic-url");
    assert!(data[0]["call"].as_str() < data[1]["call"].as_str());
    let mut decisions = Vec::new();
    for item in data {
        let candidates = item["candidates"].as_array().unwrap();
        let targets = candidates
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(!targets.is_empty() && targets.windows(2).all(|pair| pair[0] < pair[1]));
        let pick = targets[0];
        let (repo, contract) = pick.split_once("::").unwrap();
        decisions.push((
            item["call"].as_str().unwrap(),
            item["kind"].as_str().unwrap(),
            repo,
            contract,
            pick,
        ));
    }
    let text = decisions.iter().map(|(call, kind, _, _, pick)| {
        format!("[[decision]]\ncall = {call:?}\nkind = {kind:?}\ntarget = {pick:?}\nby = \"human\"\n\n")
    }).collect::<String>();
    fs::write(web.join("meada.decisions.toml"), text).unwrap();
    let applied: serde_json::Value = serde_json::from_str(&read()).unwrap();
    assert!(applied.get("open_items").is_none(), "{applied}");
    for (call, _, repo, contract, _) in decisions {
        let links: Vec<_> = applied["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["consumer"] == call)
            .collect();
        assert!(matches!(links.as_slice(), [l] if l["level"] == "decided"
            && l["contract"] == contract && l["provider"].as_str().unwrap().starts_with(&format!("{repo}::"))));
    }
    let generated: serde_json::Value =
        serde_json::from_str(&generated("items_export_many").export().unwrap()).unwrap();
    assert_eq!(
        generated["open_items"][0]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        55
    );
}

#[test]
fn a_dynamic_url_without_candidates_explains_method_path_and_missing_peers() {
    use common::fixture_repo;
    let peer = "example.com/open/api";
    let src = r#"
export function WithPath(base: string) { return fetch(`${base}/v1/orders`); }
export function WithMethod(input: { url: string }) { return fetch(input.url, { method: 'PATCH' }); }
export function WithCandidate(base: string) { return fetch(`${base}/orders`); }
"#;
    for (case, present, named) in [
        ("items_url_cause", true, true),
        ("items_url_missing_peer", false, true),
        ("items_url_own_scope", false, false),
    ] {
        let repo = fixture_repo(
            case,
            "open-few",
            Some("https://example.com/open/client.git"),
        );
        fs::remove_dir_all(repo.join("app")).unwrap();
        fs::remove_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("client.ts"), src).unwrap();
        if named {
            fs::write(
                repo.join("meada.toml"),
                format!("[[peer]]\nrepo = {peer:?}\npath = \"../http-value-flow-api\"\n"),
            )
            .unwrap();
        }
        if present {
            let api = fixture_repo(
                case,
                "http-value-flow-api",
                Some("https://example.com/open/api.git"),
            );
            let openapi = api.join("openapi.yaml");
            let content = fs::read_to_string(&openapi)
                .unwrap()
                .replace("/api/orders:", "/orders:");
            fs::write(openapi, content).unwrap();
        }
        let engine = Engine::open(&repo).unwrap();
        let items = engine.local_items().unwrap();
        assert_eq!(items.len(), 3);
        let cause = |name| {
            items
                .iter()
                .find(|i| i.id.contains(name))
                .unwrap()
                .cause
                .as_deref()
        };
        if present {
            assert!(cause("WithPath").unwrap().contains("[serve] prefix"));
            assert!(cause("WithMethod").unwrap().contains("method PATCH"));
            assert_eq!(cause("WithCandidate"), None);
        } else {
            assert!(items.iter().all(|i| i.candidates.is_empty()));
            assert!(items.iter().all(|i| {
                i.cause
                    .as_deref()
                    .unwrap()
                    .contains("no repository in the workspace serves a route")
            }));
            assert!(
                items
                    .iter()
                    .all(|i| i.cause.as_deref().unwrap().contains(peer) == named)
            );
        }
        let search = engine.search("WithPath", Some("unresolved"), None).unwrap();
        assert!(search.contains(cause("WithPath").unwrap()), "{search}");
        let (list, _, ok) = meada(&repo, &["decide", "--list"], "", |_| {});
        assert!(ok && list.contains(cause("WithPath").unwrap()), "{list}");
    }
}
