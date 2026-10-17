//! Provider documents (spec #21): OpenAPI routes of a repository with a `[serve]` table become
//! provider contracts, merged with the code routes they match, over the documented chain
//! (`common::documented_chain`).

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{assert_golden, documented_chain, fixture_repo, git, without_processes};
use meada::engine::{self, Direction, Engine, Start};

#[test]
fn document_routes_are_providers_and_merge_with_code_routes() {
    let (web, gateway, _) = documented_chain("documents_export");

    for (repo, name) in [(gateway, "gateway"), (web, "web")] {
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_golden(
            &format!("chain-documents-{name}.export.json"),
            &without_processes(&export),
        );
    }
}

#[test]
fn trace_from_web_stops_at_a_documented_only_route() {
    let (web, _, _) = documented_chain("documents_trace");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(Start::From("shipment"), Direction::Down, None)
        .unwrap();

    assert_golden("chain-documents.trace-shipment.txt", &answer);
}

#[test]
fn search_names_the_document_of_a_documented_only_route() {
    let (web, _, _) = documented_chain("documents_search");

    let answer = Engine::open(&web)
        .unwrap()
        .search("shipments", Some("route"), None)
        .unwrap();

    assert_golden("chain-documents.search-shipments.txt", &answer);
}

#[test]
fn context_on_a_merged_route_shows_its_handler_and_documented_fields() {
    let (_, gateway, _) = documented_chain("documents_context");

    let answer = Engine::open(&gateway)
        .unwrap()
        .context("example.com/chain/gateway::src/orders.ts::listOrders", None)
        .unwrap();

    assert_golden("chain-documents.context-orders.txt", &answer);
}

/// `documents-shapes`: an OpenAPI 3.0 JSON document with local `$ref`s into `components`,
/// `allOf`, `oneOf`, an array response, `additionalProperties: true`, a reference cycle and an
/// external `$ref`, plus a later document listing one of its routes again.
#[test]
fn response_schemas_are_exported_as_shapes() {
    let repo = fixture_repo(
        "documents_shapes_export",
        "documents-shapes",
        Some("https://example.com/documents/shapes.git"),
    );

    let export = Engine::open(&repo).unwrap().export().unwrap();

    assert_golden("documents-shapes.export.json", &without_processes(&export));
}

#[test]
fn status_reports_external_references() {
    let repo = fixture_repo(
        "documents_shapes_status",
        "documents-shapes",
        Some("https://example.com/documents/shapes.git"),
    );
    Engine::open(&repo).unwrap().refresh().unwrap();

    assert_golden(
        "documents-shapes.status.txt",
        &engine::status(&repo).unwrap(),
    );
}

#[test]
fn status_reports_the_documents_it_ignored() {
    let (_, gateway, _) = documented_chain("documents_status");
    Engine::open(&gateway).unwrap().refresh().unwrap();

    assert_golden(
        "chain-documents.status.txt",
        &engine::status(&gateway).unwrap(),
    );
}

/// The `(level, reason)` of the link from the web call in `function`, or `None` when it has
/// none.
fn link_of(web: &std::path::Path, function: &str) -> Option<(String, String)> {
    let export = Engine::open(web).unwrap().export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    let links = export["links"].as_array().unwrap();
    let link = links.iter().find(|l| {
        let consumer = l["consumer"].as_str().unwrap();
        consumer.contains(&format!("::{function}::"))
    })?;
    Some((
        link["level"].as_str().unwrap().into(),
        link["reason"].as_str().unwrap().into(),
    ))
}

#[test]
fn servers_urls_add_hosts_and_server_paths() {
    let (web, _, _) = documented_chain("documents_servers");
    let link = |function| link_of(&web, function);
    let inferred = |reason: &str| Some(("inferred".to_string(), reason.to_string()));

    // The public URL of the document: its host and its server path.
    assert_eq!(link("edgeShipment"), inferred("gateway prefix /gw"));
    // A server variable takes its default.
    assert_eq!(link("euShipment"), inferred("gateway prefix /v3"));
    // A relative URL gives only its path, whatever the host.
    assert_eq!(
        link("relativeShipment"),
        inferred("gateway prefix /relative/base")
    );
    // A variable without a default contributes nothing, not even its path.
    assert_eq!(link("unsetShipment"), None);
}

#[test]
fn listed_documents_are_read_even_when_ignored_or_oddly_named() {
    let (_, gateway, _) = documented_chain("documents_listed");

    let export = Engine::open(&gateway).unwrap().export().unwrap();

    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    let strings = |key: &str, field: &str| -> Vec<String> {
        let items = export[key].as_array().unwrap();
        items
            .iter()
            .map(|i| i[field].as_str().unwrap().to_string())
            .collect()
    };
    let documents = strings("documents", "path");
    // Listed and gitignored; listed under a name discovery does not know.
    assert!(documents.contains(&"generated/openapi.yaml".into()));
    assert!(documents.contains(&"docs/api-contract.yaml".into()));
    // Listed and missing: no document, no error.
    assert!(!documents.contains(&"docs/missing.yaml".into()));
    let providers = strings("providers", "contract");
    assert!(providers.contains(&"http::GET::/api/generated".into()));
    assert!(providers.contains(&"http::GET::/api/carriers".into()));
}

#[test]
fn search_finds_a_document_by_path_or_title() {
    let (web, gateway, _) = documented_chain("documents_find");
    let engine = Engine::open(&gateway).unwrap();

    let by = |query: &str| engine.search(query, Some("document"), None).unwrap();

    assert_golden(
        "chain-documents.search-document-path.txt",
        &by("orders.openapi"),
    );
    assert_golden("chain-documents.search-document-title.txt", &by("CARRIERS"));
    assert_eq!(by("nothing"), "no matches for `nothing`\n");
    // The workspace's documents, from any repository; the vendored copy of a repository
    // without `[serve]` is none.
    let answer = Engine::open(&web)
        .unwrap()
        .search("openapi", Some("document"), None)
        .unwrap();
    assert!(answer.contains("chain/gateway openapi.yaml"), "{answer}");
    assert!(!answer.contains("vendor"), "{answer}");
}

/// A repository written from `files` (path, text) at `<CARGO_TARGET_TMPDIR>/<case>/<name>`,
/// with the origin `https://example.com/acme/<name>.git` and one commit.
fn repo(case: &str, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(case).join(name);
    let _ = fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    git(&dir, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let origin = format!("https://example.com/acme/{name}.git");
    git(&dir, &["remote", "add", "origin", &origin]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "fixture"]);
    dir
}

/// A client that calls each path of `paths` from a function `call<n>`, next to its peer
/// `shop`.
fn client(case: &str, paths: &[&str]) -> PathBuf {
    let calls: String = (paths.iter().enumerate())
        .map(|(n, path)| format!("export const call{n} = () => fetch(\"{path}\");\n"))
        .collect();
    let toml = "[[peer]]\nrepo = \"example.com/acme/shop\"\n";
    repo(
        case,
        "client",
        &[("meada.toml", toml), ("src/client.ts", &calls)],
    )
}

/// An OpenAPI 3.1 document with a GET of each path, served at each of `servers`.
fn openapi(servers: &[&str], paths: &[&str]) -> String {
    let servers: String = servers.iter().map(|s| format!("  - url: {s}\n")).collect();
    let paths: String = paths
        .iter()
        .map(|p| format!("  {p}:\n    get: {{}}\n"))
        .collect();
    let servers = match servers.is_empty() {
        true => String::new(),
        false => format!("servers:\n{servers}"),
    };
    format!("openapi: 3.1.0\n{servers}paths:\n{paths}")
}

#[test]
fn each_document_gives_its_own_gateway_forms() {
    let case = "documents_gateway_forms";
    let a = openapi(&["/edge"], &["/v1/items", "/v1/a-only", "/v1/orders"]);
    let b = openapi(&[], &["/v1/items", "/v1/b-only"]);
    let route = "export async function GET() {\n  return Response.json({});\n}\n";
    repo(
        case,
        "shop",
        &[
            (
                "meada.toml",
                "[serve]\nprefix = \"/shop\"\nstrip = \"/v1\"\n",
            ),
            ("a.openapi.yaml", &a),
            ("b.openapi.yaml", &b),
            // A code route: it merges with the route of `a.openapi.yaml`.
            ("app/v1/orders/route.ts", route),
        ],
    );
    let paths = [
        "/edge/items",
        "/shop/items",
        "/edge/a-only",
        "/shop/a-only",
        "/shop/b-only",
        "/edge/b-only",
        "/shop/orders",
        "/edge/orders",
    ];
    let client = client(case, &paths);

    let link = |n: usize| link_of(&client, &format!("call{n}"));

    let inferred = |reason: &str| Some(("inferred".to_string(), reason.to_string()));
    let edge = inferred("gateway prefix /edge, gateway strip /v1");
    let shop = inferred("gateway prefix /shop, gateway strip /v1");
    // A route two documents list: the server path of one, the `[serve]` prefix of the other,
    // which has no server path.
    assert_eq!(link(0), edge);
    assert_eq!(link(1), shop);
    // Only a document with a server path lists it: the server path replaces the prefix.
    assert_eq!(link(2), edge);
    assert_eq!(link(3), None);
    // Only a document without server paths lists it: the `[serve]` prefix.
    assert_eq!(link(4), shop);
    assert_eq!(link(5), None);
    // A code route keeps its `[serve]` form and adds the document's.
    assert_eq!(link(6), shop);
    assert_eq!(link(7), edge);
}

#[test]
fn documents_alone_make_a_serve_table_and_unlisted_ignored_ones_stay_out() {
    let case = "documents_listed_only";
    let shop = repo(
        case,
        "shop",
        &[
            (
                "meada.toml",
                "[serve]\ndocuments = [\"spec/contract.yaml\"]\n",
            ),
            (".gitignore", "build/\nspec/\n"),
            ("spec/contract.yaml", &openapi(&[], &["/listed"])),
            ("build/openapi.yaml", &openapi(&[], &["/ignored"])),
        ],
    );

    let export = Engine::open(&shop).unwrap().export().unwrap();

    assert!(export.contains("http::GET::/listed"), "{export}");
    assert!(!export.contains("/ignored"), "{export}");
    assert!(!export.contains("build/openapi.yaml"), "{export}");
}

#[cfg(unix)]
#[test]
fn a_listed_document_behind_a_symlink_is_not_read() {
    use std::os::unix::fs::symlink;

    let case = "documents_symlink";
    let outside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(case)
        .join("outside");
    let _ = fs::remove_dir_all(&outside);
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("openapi.yaml"), openapi(&[], &["/secret"])).unwrap();
    let toml = "[serve]\ndocuments = [\"linked.yaml\", \"dir/openapi.yaml\"]\n";
    let shop = repo(case, "shop", &[("meada.toml", toml)]);
    symlink(outside.join("openapi.yaml"), shop.join("linked.yaml")).unwrap();
    symlink(&outside, shop.join("dir")).unwrap();

    let export = Engine::open(&shop).unwrap().export().unwrap();
    let status = engine::status(&shop).unwrap();

    assert!(!export.contains("/secret"), "{export}");
    for path in ["linked.yaml", "dir/openapi.yaml"] {
        let line = format!("{path}: listed in `serve.documents`, not found");
        assert!(status.contains(&line), "{status}");
    }
}

#[test]
fn context_on_a_route_shows_its_providers_and_documented_fields() {
    let (_, gateway, _) = documented_chain("documents_context_route");
    let engine = Engine::open(&gateway).unwrap();

    let context = |route: &str| engine.context(route, None).unwrap();

    assert_golden(
        "chain-documents.context-route-shipments.txt",
        &context("http::GET::/api/shipments/{}"),
    );
    assert_golden(
        "chain-documents.context-route-orders.txt",
        &context("http::GET::/api/orders"),
    );
    assert_eq!(
        context("http::GET::/api/nothing"),
        "no http matches `http::GET::/api/nothing`\n"
    );
}
