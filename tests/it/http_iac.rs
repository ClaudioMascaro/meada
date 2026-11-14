//! Service and Ingress HTTP routing uses indexed infrastructure and workload code (#665).

use crate::common::{assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
use std::path::PathBuf;

fn workspace(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    let repo = |fixture, name| {
        fixture_repo(
            case,
            fixture,
            Some(&format!("https://example.com/http-iac/{name}.git")),
        )
    };
    (
        repo("http-iac-client", "client"),
        repo("http-iac-deploy", "deploy"),
        repo("http-iac-server", "server"),
    )
}

#[test]
fn http_base_urls_follow_services_and_ingress_to_the_running_repository() {
    let (client, _, _) = workspace("http_iac_routes");
    let engine = Engine::open(&client).unwrap();
    let exported = engine.export().unwrap();
    let data: serde_json::Value = serde_json::from_str(&exported).unwrap();
    let links = data["links"].as_array().unwrap();
    assert_eq!(links.len(), 4, "{links:#?}");
    for link in links
        .iter()
        .filter(|l| l["contract"].as_str().unwrap().starts_with("http::"))
    {
        assert_eq!(
            link["provider"],
            "example.com/http-iac/server::app/orders/route.ts::GET"
        );
        let direct = link["consumer"]
            .as_str()
            .unwrap()
            .contains("::directOrders::");
        assert_eq!(link["level"], if direct { "exact" } else { "inferred" });
        if !direct {
            assert!(
                link["reason"]
                    .as_str()
                    .unwrap()
                    .contains("Service/shop/orders"),
                "{link}"
            );
        }
    }
    let context = engine.context("workloadOrders", None).unwrap();
    assert!(
        context.contains("inferred") && context.contains("Service/shop/orders"),
        "{context}"
    );
    let trace = engine
        .trace(Start::From("publicOrders"), Direction::Down, None)
        .unwrap();
    assert!(
        trace.contains("Ingress.networking.k8s.io/shop/orders"),
        "{trace}"
    );
    assert_golden("http-iac-client.export.json", &exported);
    assert_golden("http-iac-client.context.txt", &context);
    assert_golden("http-iac-client.trace.txt", &trace);
}

fn links(client: &std::path::Path) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&Engine::open(client).unwrap().export().unwrap())
        .unwrap()["links"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["contract"].as_str().unwrap().starts_with("http::"))
        .cloned()
        .collect()
}

#[test]
fn ingress_rewrites_unknown_backends_and_longer_unread_mounts_do_not_guess() {
    let (client, deploy, _) = workspace("http_iac_unread");
    let path = deploy.join("workloads.yaml");
    let original = std::fs::read_to_string(&path).unwrap();
    for (name, source) in [
        ("rewrite", original.replace("metadata: {name: orders, namespace: shop}\nspec:\n  rules:", "metadata:\n  name: orders\n  namespace: shop\n  annotations: {nginx.ingress.kubernetes.io/rewrite-target: /}\nspec:\n  rules:")),
        ("unknown backend", original.replace("service: {name: orders, port:", "service: {name: absent, port:")),
        ("unsupported path type", original.replace("pathType: Prefix", "pathType: ImplementationSpecific")),
        ("longer unread rule", format!("{original}\n          - path: /store/orders\n            pathType: Prefix\n            backend:\n              service: {{name: absent, port: {{number: 8080}}}}\n")),
    ] {
        std::fs::write(&path, source).unwrap();
        let found = links(&client);
        assert_eq!(found.len(), 2, "{name}: {found:#?}");
        assert!(found.iter().all(|l| !l["consumer"].as_str().unwrap().contains("::publicOrders::")), "{name}: {found:#?}");
    }
    // Without a pathType, each controller decides how a path matches: an unread mount too.
    std::fs::write(
        &path,
        original.replace("            pathType: Prefix\n", ""),
    )
    .unwrap();
    assert_eq!(links(&client).len(), 2);
}

#[test]
fn compose_names_use_the_callers_network_and_safe_build_path() {
    let (client, deploy, _) = workspace("http_iac_compose");
    std::fs::remove_file(deploy.join("workloads.yaml")).unwrap();
    for dir in ["caller", "inventory", "outside"] {
        std::fs::create_dir(client.join(dir)).unwrap();
    }
    std::fs::write(
        client.join("caller/client.ts"),
        "export function inventory() { return fetch(`${process.env.LOCAL_URL}/stock`); }\n",
    )
    .unwrap();
    let provider = "import express from 'express';\nconst app = express();\napp.get('/stock', (req, res) => res.json({ok: true}));\n";
    for dir in ["inventory", "outside"] {
        std::fs::write(client.join(format!("{dir}/server.ts")), provider).unwrap();
    }
    let compose = "services:\n  caller:\n    build: ./caller\n    environment:\n      LOCAL_URL: http://inventory.api_v2:7000\n  inventory.API_v2:\n    build: ./inventory\n";
    std::fs::write(client.join("compose.yaml"), compose).unwrap();
    let found = links(&client);
    let local = found
        .iter()
        .find(|l| l["consumer"].as_str().unwrap().contains("::inventory::"))
        .unwrap();
    assert_eq!(
        local["provider"], "example.com/http-iac/client::inventory/server.ts::GET /stock",
        "{found:#?}"
    );
    assert_eq!(local["level"], "inferred");
    assert!(
        local["reason"]
            .as_str()
            .unwrap()
            .contains("services.inventory.API_v2")
    );
    std::fs::write(
        client.join("compose.yaml"),
        compose.replace(
            "    build: ./inventory",
            "    build: ./inventory\n    networks: [isolated]",
        ),
    )
    .unwrap();
    assert!(
        links(&client)
            .iter()
            .all(|l| !l["consumer"].as_str().unwrap().contains("::inventory::"))
    );
}

#[test]
fn service_selection_keeps_every_label_namespace_and_evaluation_root() {
    let (client, deploy, _) = workspace("http_iac_scope");
    let source = std::fs::read_to_string(deploy.join("workloads.yaml")).unwrap();
    let workload = source
        .split("---\n")
        .find(|s| s.contains("name: orders, namespace: shop") && s.contains("kind: Deployment"))
        .unwrap();
    // Same labels in a different namespace never satisfy the selector.
    std::fs::write(
        deploy.join("workloads.yaml"),
        source.replace(
            "selector: {app: orders}",
            "selector: {app: orders, stage: production}",
        ) + "\n---\n"
            + &workload
                .replace("namespace: shop", "namespace: elsewhere")
                .replace("app: orders}", "app: orders, stage: production}"),
    )
    .unwrap();
    let found = links(&client);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0]["consumer"]
            .as_str()
            .unwrap()
            .contains("::directOrders::")
    );
    // A matching workload in a separate evaluation root cannot complete this Service either.
    std::fs::create_dir(deploy.join("unrelated")).unwrap();
    std::fs::write(
        deploy.join("unrelated/server.yaml"),
        workload.replace("app: orders}", "app: orders, stage: production}"),
    )
    .unwrap();
    assert_eq!(links(&client).len(), 1);
}

#[test]
fn http_infrastructure_queries_match_full_reads_of_the_same_workspace() {
    use meada::engine::with_every_row;
    let (client, _, server) = workspace("http_iac_equivalence");
    for root in [&client, &server] {
        let engine = Engine::open(root).unwrap();
        for symbol in ["workloadOrders", "publicOrders", "http::GET::/orders"] {
            let query = || engine.context(symbol, Some(100_000)).unwrap();
            assert_eq!(query(), with_every_row(query), "context {symbol}");
        }
        for (start, direction) in [
            ("publicOrders", Direction::Down),
            ("http::GET::/orders", Direction::Up),
        ] {
            let query = || {
                engine
                    .trace(Start::From(start), direction, Some(100_000))
                    .unwrap()
            };
            assert_eq!(query(), with_every_row(query), "trace {start}");
        }
        let query = || {
            engine
                .search("orders", Some("route"), Some(100_000))
                .unwrap()
        };
        assert_eq!(query(), with_every_row(query), "route search");
    }
}

#[test]
fn a_valid_runner_decision_selects_the_repository_used_by_http() {
    use crate::common::git;
    let (client, deploy, server) = workspace("http_iac_runner_choice");
    let other = client.parent().unwrap().join("other");
    let _ = std::fs::remove_dir_all(&other);
    std::fs::create_dir_all(&other).unwrap();
    std::fs::create_dir_all(other.join(".github/workflows")).unwrap();
    std::fs::copy(
        server.join(".github/workflows/build.yml"),
        other.join(".github/workflows/build.yml"),
    )
    .unwrap();
    std::fs::write(
        other.join("meada.toml"),
        "[[peer]]\nrepo = 'example.com/http-iac/client'\n",
    )
    .unwrap();
    git(&other, &["init", "-q"]);
    git(
        &other,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/http-iac/other.git",
        ],
    );
    git(&other, &["add", "."]);
    git(&other, &["commit", "-qm", "second literal builder"]);
    // A raw ambiguity cannot invent which repository runs the Service's workload.
    let before = links(&client);
    assert_eq!(before.len(), 1, "{before:#?}");
    assert!(
        before[0]["consumer"]
            .as_str()
            .unwrap()
            .contains("::directOrders::")
    );
    let engine = Engine::open(&deploy).unwrap();
    let items = engine.open_items().unwrap();
    let item = items
        .iter()
        .find(|i| i.id.contains("Deployment.apps/shop/orders::image::"))
        .unwrap();
    assert_eq!(item.candidates.len(), 2);
    std::fs::write(deploy.join("meada.decisions.toml"), format!(
        "[[decision]]\nkind = 'ambiguous-link'\ncall = {}\ntarget = 'example.com/http-iac/server::image::ghcr.io/http-iac/server'\nby = 'human'\n",
        serde_json::to_string(&item.id).unwrap()
    )).unwrap();
    let selected: serde_json::Value =
        serde_json::from_str(&Engine::open(&deploy).unwrap().export().unwrap()).unwrap();
    assert!(selected.get("stale_decisions").is_none(), "{selected}");
    assert!(
        selected["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["level"] == "decided" && l["provider"] == "example.com/http-iac/server::.github/workflows/build.yml::jobs.image.steps[0]")
    );
    let after = links(&client);
    assert_eq!(after.len(), 3, "{after:#?}");
    assert!(
        after
            .iter()
            .all(|l| l["provider"] == "example.com/http-iac/server::app/orders/route.ts::GET")
    );
    let context = Engine::open(&client)
        .unwrap()
        .context("workloadOrders", None)
        .unwrap();
    assert!(context.contains("Service/shop/orders"), "{context}");
    let query = || {
        Engine::open(&client)
            .unwrap()
            .context("publicOrders", None)
            .unwrap()
    };
    assert_eq!(query(), meada::engine::with_every_row(query));
    std::fs::remove_file(deploy.join("meada.decisions.toml")).unwrap();
    assert_eq!(links(&client), before);
}

#[test]
fn an_unread_environment_base_blocks_guessing_and_an_explicit_override_wins() {
    let (client, deploy, _) = workspace("http_iac_unread_environment");
    let source = std::fs::read_to_string(deploy.join("workloads.yaml")).unwrap();
    std::fs::write(
        client.join("secret.ts"),
        "export function secretOrders() { return fetch(`${process.env.HIDDEN_URL}/orders`); }\n",
    )
    .unwrap();
    let changed = source.replace(
        "          env:\n",
        "          envFrom:\n            - secretRef: {name: missing}\n          env:\n",
    );
    std::fs::write(deploy.join("workloads.yaml"), &changed).unwrap();
    let found = links(&client);
    assert_eq!(found.len(), 3, "{found:#?}");
    assert!(
        found
            .iter()
            .all(|l| !l["consumer"].as_str().unwrap().contains("::secretOrders::"))
    );
    // An explicit name in that container overrides the unread envFrom prefix.
    std::fs::write(
        deploy.join("workloads.yaml"),
        changed.replace(
            "          env:\n",
            "          env:\n            - {name: HIDDEN_URL, value: 'http://orders:8080'}\n",
        ),
    )
    .unwrap();
    let found = links(&client);
    assert_eq!(found.len(), 4, "{found:#?}");
    assert!(
        found
            .iter()
            .find(|l| l["consumer"].as_str().unwrap().contains("::secretOrders::"))
            .unwrap()["reason"]
            .as_str()
            .unwrap()
            .contains("Service/shop/orders")
    );
}

#[test]
fn duplicate_service_addresses_do_not_choose_the_first_selector() {
    let (client, deploy, _) = workspace("http_iac_duplicate_service");
    let source = std::fs::read_to_string(deploy.join("workloads.yaml")).unwrap();
    std::fs::write(deploy.join("workloads.yaml"), source + "\n---\napiVersion: v1\nkind: Service\nmetadata: {name: orders, namespace: shop}\nspec: {selector: {app: other}}\n").unwrap();
    let found = links(&client);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0]["consumer"]
            .as_str()
            .unwrap()
            .contains("::directOrders::")
    );
}

#[test]
fn ingress_alias_expansion_is_bounded_before_long_paths_are_repeated() {
    let (client, deploy, server) = workspace("http_iac_alias_budget");
    let mut routes = "import express from 'express';\nconst app = express();\n".to_string();
    for i in 0..3 {
        routes.push_str(&format!(
            "app.get('/{}-{i}', (req, res) => res.json({{ok: true}}));\n",
            "x".repeat(100_000)
        ));
    }
    std::fs::write(server.join("long.ts"), routes).unwrap();
    let source = std::fs::read_to_string(deploy.join("workloads.yaml")).unwrap();
    let mut paths = String::new();
    for i in 0..63 {
        paths.push_str(&format!("          - path: /other-{i}\n            pathType: Prefix\n            backend:\n              service: {{name: orders, port: {{number: 8080}}}}\n"));
    }
    std::fs::write(deploy.join("workloads.yaml"), source + &paths).unwrap();
    let start = std::time::Instant::now();
    let data: serde_json::Value =
        serde_json::from_str(&Engine::open(&server).unwrap().export().unwrap()).unwrap();
    assert_eq!(
        data["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["contract"].as_str().unwrap().starts_with("http::"))
            .count(),
        4,
        "all long source routes are present"
    );
    let found = links(&client);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(
        found
            .iter()
            .all(|l| !l["consumer"].as_str().unwrap().contains("::publicOrders::"))
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(15),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn http_routing_waits_for_the_image_kinds_builder_pattern_choice() {
    let (client, deploy, server) = workspace("http_iac_builder_pattern");
    std::fs::write(server.join(".github/workflows/build.yml"), "jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/http-iac/server-${{ matrix.region }}\n").unwrap();
    let source = std::fs::read_to_string(deploy.join("workloads.yaml")).unwrap();
    std::fs::write(deploy.join("workloads.yaml"), source.replace("ghcr.io/http-iac/server:v1", "ghcr.io/http-iac/server-east") + "\n---\napiVersion: v1\nkind: Pod\nmetadata: {name: unrelated, namespace: shop}\nspec:\n  containers: [{name: main, image: ghcr.io/http-iac/server-west}]\n").unwrap();
    let before = links(&client);
    assert_eq!(before.len(), 1, "{before:#?}");
    let engine = Engine::open(&server).unwrap();
    let items = engine.open_items().unwrap();
    let item = items
        .iter()
        .find(|i| i.id.starts_with("image-builder::"))
        .unwrap();
    assert_eq!(item.candidates.len(), 2);
    let chosen = item
        .candidates
        .iter()
        .find(|c| c.contract.ends_with("server-east"))
        .unwrap();
    std::fs::write(
        server.join("meada.decisions.toml"),
        format!(
            "[[decision]]\nkind = 'ambiguous-link'\ncall = {}\ntarget = {}\nby = 'human'\n",
            serde_json::to_string(&item.id).unwrap(),
            serde_json::to_string(&format!("{}::{}", chosen.repository, chosen.contract)).unwrap()
        ),
    )
    .unwrap();
    let found = links(&client);
    assert_eq!(found.len(), 3, "{found:#?}");
    assert!(
        found
            .iter()
            .all(|l| l["provider"] == "example.com/http-iac/server::app/orders/route.ts::GET")
    );
    let query = || {
        Engine::open(&client)
            .unwrap()
            .context("publicOrders", None)
            .unwrap()
    };
    let answer = query();
    assert!(answer.contains("Service/shop/orders"), "{answer}");
    assert_eq!(answer, meada::engine::with_every_row(query));
    std::fs::remove_file(server.join("meada.decisions.toml")).unwrap();
    assert_eq!(links(&client), before);
}

#[test]
fn a_declared_host_keeps_its_link_when_no_service_or_ingress_reaches_code() {
    let (client, deploy, server) = workspace("http_iac_declared_hosts");
    std::fs::write(
        server.join("meada.toml"),
        "[serve]\nhosts = [\"orders.example.com\", \"orders\"]\n",
    )
    .unwrap();
    std::fs::write(
        client.join("internal.ts"),
        "export function internalOrders() {\n  return fetch('http://orders/orders');\n}\n",
    )
    .unwrap();
    // The Service selects no workload, and the Ingress of the other declared host has a path
    // type Meada does not read: neither reaches code, so `[serve] hosts` answers as before.
    let source = std::fs::read_to_string(deploy.join("workloads.yaml")).unwrap();
    std::fs::write(
        deploy.join("workloads.yaml"),
        source.replace("selector: {app: orders}", "selector: {app: retired}")
            + "---\napiVersion: networking.k8s.io/v1\nkind: Ingress\nmetadata: {name: direct, namespace: shop}\nspec:\n  rules:\n    - host: orders.example.com\n      http:\n        paths:\n          - path: /\n            pathType: ImplementationSpecific\n            backend:\n              service: {name: orders, port: {number: 8080}}\n",
    )
    .unwrap();
    let found = links(&client);
    for (name, host) in [
        ("internalOrders", "orders"),
        ("directOrders", "orders.example.com"),
    ] {
        let link = found
            .iter()
            .find(|l| {
                l["consumer"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("::{name}::"))
            })
            .unwrap_or_else(|| panic!("{name}: {found:#?}"));
        assert_eq!(link["level"], "exact", "{link}");
        assert!(
            link["reason"]
                .as_str()
                .unwrap()
                .starts_with(&format!("host {host} is served by")),
            "{link}"
        );
    }
}

/// Overlays of one base repeat its Ingress per environment: one provider behind every copy is
/// no ambiguity, while a route as long that reaches no code still is.
#[test]
fn overlays_repeating_one_route_to_one_provider_leave_it_inferred() {
    let (client, deploy, _) = workspace("http_iac_overlays");
    std::fs::create_dir(deploy.join("base")).unwrap();
    std::fs::rename(
        deploy.join("workloads.yaml"),
        deploy.join("base/workloads.yaml"),
    )
    .unwrap();
    std::fs::write(
        deploy.join("base/kustomization.yaml"),
        "resources:\n  - workloads.yaml\n",
    )
    .unwrap();
    for stage in ["dev", "prod"] {
        let dir = deploy.join("overlays").join(stage);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("kustomization.yaml"),
            format!("namespace: shop-{stage}\nresources:\n  - ../../base\n"),
        )
        .unwrap();
    }
    let public = |found: &[serde_json::Value]| {
        let public = (found.iter())
            .filter(|l| l["consumer"].as_str().unwrap().contains("::publicOrders::"))
            .collect::<Vec<_>>();
        assert_eq!(public.len(), 1, "{found:#?}");
        public[0]["level"].as_str().unwrap().to_string()
    };
    let found = links(&client);
    assert_eq!(found.len(), 3, "{found:#?}");
    assert_eq!(public(&found), "inferred");
    let path = deploy.join("base/workloads.yaml");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{source}          - path: /store/(.*)\n            pathType: Prefix\n            backend:\n              service: {{name: orders, port: {{number: 8080}}}}\n")).unwrap();
    assert_eq!(public(&links(&client)), "ambiguous");
}
