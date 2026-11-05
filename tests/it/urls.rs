//! URLs as real code builds them (spec #3, HTTP contract kind): paths in imported constants,
//! client wrappers around `fetch`, Express routers under mount prefixes, inline route
//! handlers, and links Meada can only infer, leave ambiguous or leave out.

use crate::common;

use std::path::PathBuf;

use common::{assert_golden, fixture_repo, without_processes};
use meada::engine::{Direction, Engine, Start};

/// The Next.js provider of `tests/fixtures/http-api`, in the case directory.
fn api(case: &str) -> PathBuf {
    fixture_repo(case, "http-api", Some("https://example.com/acme/api.git"))
}

#[test]
fn constants_fold_into_urls_across_files() {
    let case = "urls_constants";
    let paths = fixture_repo(
        case,
        "http-paths",
        Some("https://example.com/acme/paths.git"),
    );
    api(case);

    let export = Engine::open(&paths).unwrap().export().unwrap();

    assert_golden("http-paths.export.json", &without_processes(&export));
}

/// The Express provider of `tests/fixtures/http-express`, in the case directory.
fn backend(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "http-express",
        Some("https://example.com/acme/backend.git"),
    )
}

/// The client wrapper consumer of `tests/fixtures/http-wrappers`, in the case directory.
fn wrappers(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "http-wrappers",
        Some("https://example.com/acme/admin.git"),
    )
}

#[test]
fn wrapper_call_sites_are_consumers_through_two_levels_and_a_cycle() {
    let case = "urls_wrappers";
    let admin = wrappers(case);
    backend(case);

    let export = Engine::open(&admin).unwrap().export().unwrap();

    assert_golden("http-wrappers.export.json", &without_processes(&export));
}

#[test]
fn wrapper_expansion_climbs_only_calls_that_resolved_exactly() {
    let repo = fixture_repo(
        "urls_wrappers_by_name",
        "http-wrappers-by-name",
        Some("https://example.com/acme/portal.git"),
    );

    let export = Engine::open(&repo).unwrap().export().unwrap();

    let json: serde_json::Value = serde_json::from_str(&export).unwrap();
    let mut consumers: Vec<_> = (json["consumers"].as_array().unwrap())
        .iter()
        .filter_map(|c| c["contract"].as_str())
        .collect();
    consumers.sort();
    // `this.get("/orders")` is exact; `store.get("/reports")` (receiver of unknown class),
    // `api.get("/list")` (a `new Api()` constant, still a name match) and `fetchJson("/bare")`
    // (no import binds it) are name matches, so none names a path (the wrappers read where they
    // stand have no readable contract).
    assert_eq!(consumers, ["http::GET::/orders"]);
}

#[test]
fn express_routes_take_mount_prefixes_through_imports_at_two_depths() {
    let case = "urls_express";
    let backend = backend(case);
    wrappers(case);

    let export = Engine::open(&backend).unwrap().export().unwrap();

    assert_golden("http-express.export.json", &without_processes(&export));
}

#[test]
fn inline_handlers_are_definitions_named_by_method_and_full_path() {
    let engine = Engine::open(&backend("urls_inline")).unwrap();

    assert_golden(
        "http-express.search-inline.txt",
        &engine.search("GET /api/users", None, None).unwrap(),
    );
    assert_golden(
        "http-express.context-inline.txt",
        &engine
            .context("register.GET /api/users/{}#1", None)
            .unwrap(),
    );
}

/// The consumer of `tests/fixtures/http-client`, with both providers next to it.
fn client(case: &str) -> Engine {
    let client = fixture_repo(
        case,
        "http-client",
        Some("https://example.com/acme/client.git"),
    );
    api(case);
    backend(case);
    Engine::open(&client).unwrap()
}

#[test]
fn hostless_urls_link_inferred_to_the_one_repository_serving_them_else_ambiguous() {
    let export = client("urls_hostless").export().unwrap();

    assert_golden("http-client.export.json", &without_processes(&export));
}

#[test]
fn an_unresolved_consumer_shows_in_search_and_context_but_never_links() {
    let engine = client("urls_unresolved");

    assert_golden(
        "http-client.search-unresolved.txt",
        &engine.search("unresolved", Some("contract"), None).unwrap(),
    );
    assert_golden(
        "http-client.context-dynamic.txt",
        &engine.context("dynamic", None).unwrap(),
    );
}

#[test]
fn destructured_parameters_fold_like_positional_ones() {
    let case = "urls_destructured";
    let reports = fixture_repo(
        case,
        "http-destructured",
        Some("https://example.com/acme/portal.git"),
    );

    let export = Engine::open(&reports).unwrap().export().unwrap();

    assert_golden("http-destructured.export.json", &without_processes(&export));
}

#[test]
fn a_parameter_defaulting_to_fetch_is_a_fetch_call() {
    let case = "urls_fetch_param";
    let client = fixture_repo(
        case,
        "http-fetch-param",
        Some("https://example.com/acme/portal.git"),
    );

    let export = Engine::open(&client).unwrap().export().unwrap();

    assert_golden("http-fetch-param.export.json", &without_processes(&export));
}

#[test]
fn a_route_of_every_method_links_a_consumer_of_any_method() {
    let case = "urls_any_method";
    let client = fixture_repo(
        case,
        "http-any-client",
        Some("https://example.com/acme/client.git"),
    );
    fixture_repo(
        case,
        "conformance-python",
        Some("https://example.com/acme/conformance.git"),
    );
    let engine = Engine::open(&client).unwrap();

    assert_golden(
        "http-any-client.export.json",
        &without_processes(&engine.export().unwrap()),
    );
    // A route named with any method starts at the provider of every method.
    let up = engine.trace(Start::From("POST /health"), Direction::Up, None);
    assert_golden("http-any-client.trace-up.txt", &up.unwrap());
    let items = engine.open_items().unwrap();
    // The peer's own unresolved calls are items too: only the client's are read here.
    let candidates: Vec<&str> = (items
        .iter()
        .filter(|i| i.repository == "example.com/acme/client"))
    .flat_map(|i| &i.candidates)
    .map(|c| c.contract.as_str())
    .collect();
    assert_eq!(
        candidates,
        [
            "http::ANY::/about",
            "http::ANY::/contact",
            "http::ANY::/health",
            "http::ANY::/vintage/{}",
            "http::PUT::/members/{}"
        ]
    );
}

#[test]
fn a_spring_route_of_every_method_links_a_consumer_of_any_method() {
    let case = "urls_spring_any_method";
    let client = fixture_repo(
        case,
        "http-spring-client",
        Some("https://example.com/acme/client.git"),
    );
    fixture_repo(
        case,
        "conformance-java",
        Some("https://example.com/acme/conformance.git"),
    );

    let export = Engine::open(&client).unwrap().export().unwrap();

    let json: serde_json::Value = serde_json::from_str(&export).unwrap();
    let links: Vec<(&str, &str, &str)> = (json["links"].as_array().unwrap().iter())
        .map(|l| {
            let text = |key| l[key].as_str().unwrap();
            (text("contract"), text("level"), text("reason"))
        })
        .collect();
    let peer = "the only repository serving it is example.com/acme/conformance";
    assert_eq!(
        links,
        [
            (
                "http::DELETE::/api/orders/any",
                "inferred",
                &format!("host unresolved; {peer}; any method")[..]
            ),
            (
                "http::PUT::/api/orders/{}",
                "inferred",
                &format!("host unresolved; {peer}")[..]
            ),
        ]
    );
}

#[test]
fn a_java_consumer_through_a_wrapper_links_a_spring_route_of_a_peer() {
    let case = "urls_java_client";
    let client = fixture_repo(
        case,
        "http-java-client",
        Some("https://example.com/acme/client.git"),
    );
    fixture_repo(
        case,
        "conformance-java",
        Some("https://example.com/acme/conformance.git"),
    );

    let export = Engine::open(&client).unwrap().export().unwrap();

    let json: serde_json::Value = serde_json::from_str(&export).unwrap();
    let links: Vec<(&str, &str, &str)> = (json["links"].as_array().unwrap().iter())
        .map(|l| {
            let text = |key| l[key].as_str().unwrap();
            (text("contract"), text("level"), text("reason"))
        })
        .collect();
    let peer = "the only repository serving it is example.com/acme/conformance";
    assert_eq!(
        links,
        [
            (
                "http::DELETE::/api/orders/any",
                "inferred",
                &format!("host unresolved; {peer}; any method")[..]
            ),
            (
                "http::PUT::/api/orders/{}",
                "inferred",
                &format!("host unresolved; {peer}")[..]
            ),
        ]
    );
}

#[test]
fn a_url_a_function_only_builds_folds_at_the_call_and_through_a_constant() {
    let case = "urls_builder";
    let repo = fixture_repo(
        case,
        "http-url-builder",
        Some("https://example.com/acme/vets-web.git"),
    );
    fixture_repo(
        case,
        "http-url-builder-api",
        Some("https://example.com/acme/vets-api.git"),
    );

    let export = Engine::open(&repo).unwrap().export().unwrap();

    assert_golden("http-url-builder.export.json", &without_processes(&export));
    let json: serde_json::Value = serde_json::from_str(&export).unwrap();
    let linked = |file: &str| -> Vec<String> {
        let mut out: Vec<String> = (json["links"].as_array().unwrap())
            .iter()
            .filter(|l| l["consumer"].as_str().is_some_and(|c| c.contains(file)))
            .map(|l| l["contract"].as_str().unwrap().to_string())
            .collect();
        out.sort();
        out
    };
    // The builder imported, called in the argument, held by a constant, and with a lone `return`.
    assert_eq!(
        linked("src/pages.ts"),
        [
            "http::GET::/api/owners",
            "http::GET::/api/owners",
            "http::GET::/api/vets",
        ]
    );
    assert_eq!(linked("src/local.ts"), ["http::GET::/api/vets"]);
    assert_eq!(linked("src/plain.js"), ["http::GET::/api/vets"]);
    // An arrow function with a block body folds when the block is one `return` and not otherwise.
    assert_eq!(linked("src/block.ts"), ["http::GET::/api/vets"]);
    // `new URL(x)` of one argument is `x` (WHATWG URL Standard), through `toString()`, `href`,
    // `String(u)`, a template string and a client wrapper; `searchParams`, `search` and `hash`
    // keep the path.
    assert_eq!(
        linked("src/whatwg.ts"),
        [
            "http::GET::/api/owners",
            "http::GET::/api/owners",
            "http::GET::/api/owners",
            "http::GET::/api/vets",
            "http::GET::/api/vets",
            "http::GET::/api/vets",
        ]
    );
    // `new URL(reference, base)` is the join of the two (RFC 3986 section 5.2). Over a literal base
    // an absolute reference is itself, `//host/x` takes its host, `/x` replaces the path and `x`
    // joins the base's directory, in the argument, through a constant and a client wrapper. Over
    // an environment read or a hole an absolute path keeps the lead; the rest is unresolved.
    let consumers: std::collections::BTreeMap<String, Option<&str>> =
        (json["consumers"].as_array().unwrap())
            .iter()
            .filter(|c| c["site"].as_str().unwrap().contains("src/join.ts"))
            .map(|c| {
                let site = c["site"].as_str().unwrap().split("::").collect::<Vec<_>>();
                (site[2..].join("::"), c["contract"].as_str())
            })
            .collect();
    let vets = Some("http::GET::/api/vets");
    let owners = Some("http::GET::/api/owners");
    let expected = [
        ("absolute::fetch#0", vets),
        ("absolutePath::fetch#0", vets),
        ("envAbsolutePath::fetch#0", vets),
        ("envNetworkPath::fetch#0", None),
        ("envReference::fetch#0", None),
        ("envRelative::fetch#0", None),
        ("fragmentOnly::fetch#0", None),
        ("holeAbsolutePath::fetch#0", vets),
        ("holeBareAbsolutePath::fetch#0", owners),
        ("holeBareRelative::fetch#0", None),
        ("networkPath::fetch#0", owners),
        ("queryOnly::fetch#0", None),
        ("relativeUnderSlash::fetch#0", owners),
        ("relativeWithoutSlash::fetch#0", vets),
        ("throughConstant::fetch#0", owners),
        ("wrapped::from#0", vets),
        ("wrapped::get#0", owners),
    ];
    let expected: std::collections::BTreeMap<_, _> =
        expected.map(|(k, v)| (k.to_string(), v)).into();
    assert_eq!(consumers, expected);
    assert_eq!(linked("src/join.ts").len(), 11);
    // A base read from the environment, a body of two statements, a name match and a name a
    // parameter, a `catch` binding or a local hides fold nothing; neither does a `new URL`
    // written through its path or host, nor a `URL` the file declares or imports from the
    // repository.
    for file in [
        "src/env.ts",
        "src/statements.ts",
        "src/names.ts",
        "src/hidden.ts",
        "src/written.ts",
        "src/declared.ts",
        "src/own.ts",
    ] {
        assert_eq!(linked(file), Vec::<String>::new(), "{file}");
    }
}

/// Backstage (#615): each backend plugin's router is served under `/api/<pluginId>`, and a URL
/// from `discovery.getBaseUrl('<pluginId>')` starts there; a plugin whose `pluginId` is no
/// literal keeps its routes at the root.
#[test]
fn backstage_plugins_serve_under_api_and_their_plugin_id() {
    let repo = fixture_repo(
        "urls_backstage",
        "http-backstage",
        Some("https://example.com/acme/backstage.git"),
    );

    let export = Engine::open(&repo).unwrap().export().unwrap();

    let json: serde_json::Value = serde_json::from_str(&export).unwrap();
    let mut providers: Vec<&str> = (json["providers"].as_array().unwrap())
        .iter()
        .filter_map(|p| p["contract"].as_str())
        .collect();
    providers.sort();
    // `legacy` reads its `pluginId` from the environment, `shared` imports it and `chained` binds
    // it to another constant: their routers stay at the root.
    assert_eq!(
        providers,
        [
            "http::GET::/api/approval/requests/{}",
            "http::GET::/api/catalog/entities",
            "http::GET::/api/orders/requests/{}",
            "http::GET::/chain",
            "http::GET::/health",
            "http::GET::/ping",
        ]
    );
    let links: Vec<_> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            let consumer = l["consumer"].as_str().unwrap();
            let consumer = consumer.rsplit_once("clients.ts::").unwrap().1;
            let provider = l["provider"].as_str().unwrap();
            (
                consumer,
                provider.rsplit_once("::").unwrap().1,
                l["level"].as_str(),
            )
        })
        .collect();
    let approval = "createRouter.GET /api/approval/requests/{}";
    let orders = "createRouter.GET /api/orders/requests/{}";
    assert_eq!(
        links,
        [
            ("getApproval::fetch#0", approval, Some("inferred")),
            (
                "getApprovalFromBrowser::fetch#0",
                approval,
                Some("inferred")
            ),
            ("getOrder::fetch#0", orders, Some("inferred")),
            (
                "listEntities::fetch#0",
                "GET /api/catalog/entities",
                Some("inferred"),
            ),
        ]
    );
    // Each names the mount: through `getBaseUrl`, or by a route the plugin's mount serves.
    for link in json["links"].as_array().unwrap() {
        let reason = link["reason"].as_str().unwrap();
        let plugin = ["orders", "catalog", "approval"]
            .into_iter()
            .find(|p| reason.contains(&format!("plugin {p}")))
            .unwrap();
        let mount = format!("host unresolved (Backstage plugin {plugin} at /api/{plugin})");
        assert!(reason.starts_with(&mount), "{reason}");
    }
    // A `getBaseUrl` of something other than the discovery service folds nothing, and a path
    // under a mount that no route of the plugin serves names no plugin.
    let consumer = |name: &str| {
        let all = json["consumers"].as_array().unwrap();
        let site = |c: &&serde_json::Value| c["site"].as_str().unwrap().contains(name);
        let c = all.iter().find(site).unwrap();
        (c["contract"].as_str().unwrap(), c["via"].as_str())
    };
    assert_eq!(
        consumer("::getSetting::"),
        ("http::GET::/requests/{}", None)
    );
    assert_eq!(
        consumer("::listOrderNotes::"),
        ("http::GET::/api/orders/notes", None)
    );
    assert_eq!(consumer("::withHeader::"), ("http::GET::/requests/1", None));
    // `meada status` names each plugin it left at the root, and no other.
    let status = meada::engine::status(&repo).unwrap();
    let left: Vec<&str> = (status.lines())
        .filter(|l| l.contains("Backstage"))
        .map(str::trim)
        .collect();
    let line = |path: &str| {
        format!(
            "plugins/{path}/src/plugin.ts::createBackendPlugin#0: Backstage plugin whose pluginId \
             is neither a string literal nor a const of its file bound to one: its routes stay at \
             the root, not under /api/<pluginId>"
        )
    };
    assert_eq!(
        left,
        ["chained-backend", "legacy-backend", "shared-backend"].map(line),
        "{status}"
    );
}

/// Backstage's proxy plugin (#616): `getBaseUrl('proxy')` plus `/<endpoint>` and
/// `/api/proxy/<endpoint>` are calls to the `target` of the endpoint in `app-config*.yaml`. A
/// `${VAR}` target goes through the peer's `base_url_env`, a literal URL is a host, and any
/// other target leaves the call unlinked, which `meada status` says.
#[test]
fn proxy_endpoints_send_a_call_to_the_target_the_app_config_gives() {
    let case = "urls_backstage_proxy";
    let web = fixture_repo(
        case,
        "ts-backstage-proxy",
        Some("https://example.com/backstage/web.git"),
    );
    fixture_repo(
        case,
        "go-backstage-inventory",
        Some("https://example.com/backstage/inventory.git"),
    );

    let export = Engine::open(&web).unwrap().export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&export).unwrap();
    let function = |site: &str| site.split("::").nth(2).unwrap().to_string();
    let mut consumers: Vec<_> = (json["consumers"].as_array().unwrap())
        .iter()
        .map(|c| {
            let site = function(c["site"].as_str().unwrap());
            (site, c["contract"].as_str().unwrap(), c["via"].as_str())
        })
        .collect();
    consumers.sort();
    let inventory = Some("env:INVENTORY_URL through proxy endpoint /inventory");
    let said = " (its target or the path it gets is not read: a target neither ${VAR} nor an http(s) URL, a pathRewrite that does more than strip the endpoint, ignorePath, prependPath, or app-config files that disagree)";
    let legacy = format!("client:Backstage proxy endpoint /legacy{said}");
    let (rewritten, slashed, twice) = (
        format!("client:Backstage proxy endpoint /rewritten{said}"),
        format!("client:Backstage proxy endpoint /slashed{said}"),
        format!("client:Backstage proxy endpoint /twice{said}"),
    );
    let (ignored, disputed) = (
        format!("client:Backstage proxy endpoint /ignored{said}"),
        format!("client:Backstage proxy endpoint /disputed{said}"),
    );
    let stock = |endpoint: &str| {
        Some(format!(
            "host:stock.example.com through proxy endpoint {endpoint}"
        ))
    };
    let (stripped_via, slashed_via) = (stock("/stripped"), stock("/slashed"));
    let mut expected = [
        // `/inventoryx` is no `/inventory`.
        (
            "otherEndpoint",
            "http::GET::/api/proxy/inventoryx/clusters",
            None,
        ),
        ("viaBrowserPath", "http::GET::/clusters", inventory),
        ("viaDiscovery", "http::GET::/clusters", inventory),
        (
            "viaLiteralTarget",
            "http::GET::/clusters",
            Some("host:stock.example.com through proxy endpoint /stock"),
        ),
        ("viaTemplate", "http::GET::/clusters", inventory),
        (
            "viaUnreadAgain",
            "http::GET::/api/proxy/legacy/clusters",
            Some(legacy.as_str()),
        ),
        (
            "viaUnreadTarget",
            "http::GET::/api/proxy/legacy/clusters",
            Some(legacy.as_str()),
        ),
        // A `pathRewrite` that only strips the endpoint is what the proxy does anyway: the call
        // goes to the stripped path. Any other rewrite changes the path the target sees, which
        // Meada does not evaluate; `/slashed` strips `<ep>/`, so a path without it is no strip.
        (
            "viaStripped",
            "http::GET::/clusters",
            stripped_via.as_deref(),
        ),
        ("viaSlashed", "http::GET::/clusters", slashed_via.as_deref()),
        (
            "viaSlashedBare",
            "http::GET::/api/proxy/slashed",
            Some(slashed.as_str()),
        ),
        (
            "viaRewrite",
            "http::GET::/api/proxy/rewritten/clusters",
            Some(rewritten.as_str()),
        ),
        (
            "viaTwoRules",
            "http::GET::/api/proxy/twice/clusters",
            Some(twice.as_str()),
        ),
        // `ignorePath` drops the path; a target `app-config.production.yaml` gives otherwise is
        // not known to be the one deployed (`/stock`, the same in both, is).
        (
            "viaIgnorePath",
            "http::GET::/api/proxy/ignored/clusters",
            Some(ignored.as_str()),
        ),
        (
            "viaDisputed",
            "http::GET::/api/proxy/disputed/clusters",
            Some(disputed.as_str()),
        ),
        // A part Meada cannot read in the endpoint, or right after it, names no endpoint.
        ("holeAfterEndpoint", "http::GET::/api/proxy/{}", None),
        ("holeInEndpoint", "http::GET::/api/proxy/{}/clusters", None),
    ];
    expected.sort();
    assert_eq!(consumers, expected.map(|(f, c, v)| (f.to_string(), c, v)));

    // The two calls of the issue and the template link to the peer; the unread target links
    // nothing.
    let links: Vec<_> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            let consumer = function(l["consumer"].as_str().unwrap());
            let provider = l["provider"].as_str().unwrap();
            (
                consumer,
                provider,
                l["level"].as_str(),
                l["reason"].as_str(),
            )
        })
        .collect();
    let peer = "example.com/backstage/inventory";
    let through = format!(
        "base URL from INVENTORY_URL through the proxy endpoint /inventory, a base_url_env of peer {peer}"
    );
    let served = |function: &str, level, reason: &str| {
        (
            function.to_string(),
            format!("{peer}::main.go::clusters"),
            Some(level),
            Some(reason.to_string()),
        )
    };
    let links: Vec<_> = (links.iter())
        .map(|(c, p, l, r)| (c.clone(), p.to_string(), *l, r.map(str::to_string)))
        .collect();
    assert_eq!(
        links,
        [
            served("viaBrowserPath", "inferred", &through),
            served("viaDiscovery", "inferred", &through),
            served(
                "viaLiteralTarget",
                "exact",
                &format!(
                    "host stock.example.com through the proxy endpoint /stock is served by {peer}"
                ),
            ),
            served(
                "viaSlashed",
                "exact",
                &format!(
                    "host stock.example.com through the proxy endpoint /slashed is served by {peer}"
                ),
            ),
            served(
                "viaStripped",
                "exact",
                &format!(
                    "host stock.example.com through the proxy endpoint /stripped is served by {peer}"
                ),
            ),
            served("viaTemplate", "inferred", &through),
        ]
    );
    let status = meada::engine::status(&web).unwrap();
    // Every line of the status: each unread endpoint once, and nothing else.
    let why = "its target or the path it gets is not read (a target neither ${VAR} nor an http(s) \
               URL, a pathRewrite that does more than strip the endpoint, ignorePath, \
               prependPath, or app-config files that disagree)";
    let unread = |endpoint: &str, calls: usize| {
        format!(
            "  Backstage proxy endpoint {endpoint}: {why}, so {calls} call(s) through it stay unlinked"
        )
    };
    let expected = [
        "example.com/backstage/web (.) indexed, 1 files".to_string(),
        unread("/disputed", 1),
        unread("/ignored", 1),
        unread("/legacy", 2),
        unread("/rewritten", 1),
        unread("/slashed", 1),
        unread("/twice", 1),
        "  config files: 2 read, 0 skipped (lockfiles: 0, unreadable: 0); values: 17, \
         redacted: 0"
            .to_string(),
        "  wrong conclusion? report it: \
         https://github.com/ClaudioMascaro/meada/issues/new?template=inference.yml"
            .to_string(),
        "example.com/backstage/inventory (../go-backstage-inventory) indexed, 1 files".to_string(),
    ];
    assert_eq!(status.lines().collect::<Vec<_>>(), expected, "{status}");
}

/// Backstage's `$include` (#689): an include Meada does not follow leaves its subtree unread, and
/// `meada status` names it with its reason, one fixture case per reason. A read through the
/// include it follows is the variable the partial names; one under an include it does not follow
/// stays a hole.
#[test]
fn backstage_includes_not_followed_are_named_with_their_reason() {
    let repo = fixture_repo(
        "urls_backstage_includes",
        "ts-backstage-includes",
        Some("https://example.com/backstage/includes.git"),
    );
    std::os::unix::fs::symlink("ok.yaml", repo.join("partials/link.yaml")).unwrap();
    // Two of these graft more than `lang::MAX_BYTES` into one root.
    let big = format!("url: u\npad: '{}'\n", "x".repeat(600 * 1024));
    std::fs::write(repo.join("partials/big.yaml"), big).unwrap();

    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&export).unwrap();
    let via = |function: &str| {
        (json["consumers"].as_array().unwrap().iter())
            .find(|c| {
                c["site"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("::{function}::"))
            })
            .map(|c| c["via"].clone())
            .unwrap_or_else(|| panic!("no consumer in {function}"))
    };
    assert_eq!(via("viaFollowed"), "env:OK_URL");
    assert_eq!(via("viaMissing"), serde_json::Value::Null);

    let status = meada::engine::status(&repo).unwrap();
    let named: Vec<&str> = (status.lines())
        .filter(|l| l.contains("Backstage $include"))
        .map(str::trim)
        .collect();
    let line = |file: &str, include: &str, why: &str| {
        format!(
            "{file}: Backstage $include {include} not followed ({why}): the keys under it stay \
             unread"
        )
    };
    let root = "app-config.yaml";
    assert_eq!(
        named,
        [
            line(
                "app-config.size.yaml",
                "./partials/big.yaml",
                "past 1 MiB of included files"
            ),
            line(root, "../outside.yaml", "a path that leaves the repository"),
            line(root, "./${ENV}/app.yaml", "a ${ in the path"),
            line(root, "./partials/broken.yaml", "a file that does not parse"),
            line(
                root,
                "./partials/gone.yaml",
                "no such file, or one ignored or hidden"
            ),
            line(root, "./partials/link.yaml", "a symbolic link"),
            line(
                root,
                "./partials/notes.txt",
                "not a .json, .yaml or .yml file"
            ),
            line(root, "./partials/ok.yaml", "another key beside $include"),
            line(
                root,
                "./partials/ok.yaml#nope",
                "a key path the file does not hold"
            ),
            line(root, "/etc/backstage/app.yaml", "an absolute path"),
            line(root, "3", "a value that is not a path"),
            line("chain/c8.yaml", "c9.yaml", "deeper than 8 includes"),
            line("partials/cycle.yaml", "../app-config.yaml", "a cycle"),
            line(
                "partials/many.yaml",
                "one.yaml",
                "more than 64 includes from one root"
            ),
        ],
        "{status}"
    );
}
