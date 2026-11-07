//! Literal IaC routes share ordinary HTTP providers and cross-repository links (#666).

use crate::common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use meada::engine::{Direction, Start, with_every_row};
use serde_json::{Value, json};

fn repository(case: &str, fixture: &str, name: &str) -> std::path::PathBuf {
    fixture_repo(
        case,
        fixture,
        Some(&format!("https://example.com/route-oracle/{name}.git")),
    )
}

#[test]
fn sam_api_and_http_api_events_are_served_by_the_local_handler() {
    let sam = repository("iac_http_sam", "iac-http-sam", "sam");
    let consumer = repository("iac_http_sam", "iac-http-consumer", "consumer");
    let exported = Engine::open(&sam).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        data["providers"],
        json!([
            {"contract": "http::GET::/orchards/{}", "site": "example.com/route-oracle/sam::runtime/app.ts::orchard"},
            {"contract": "http::POST::/orchards", "site": "example.com/route-oracle/sam::runtime/app.ts::orchard"}
        ]),
        "{exported}"
    );
    let exported = Engine::open(&consumer).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    let links = data["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{exported}");
    for link in links {
        assert_eq!(
            link["provider"], "example.com/route-oracle/sam::runtime/app.ts::orchard",
            "{link}"
        );
        assert_eq!(link["level"], "exact", "{link}");
    }
}

fn providers(repo: &std::path::Path) -> Value {
    serde_json::from_str::<Value>(&Engine::open(repo).unwrap().export().unwrap()).unwrap()["providers"].clone()
}

#[test]
fn serverless_http_and_http_api_keep_finite_routes_and_safe_handler_fallback() {
    let repo = repository("iac_http_serverless", "iac-http-serverless", "serverless");
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::PATCH::/inventory/{}", "site": "example.com/route-oracle/serverless::runtime.ts::inventory"}
        ])
    );
    std::fs::write(
        repo.join("serverless.yml"),
        r#"service: inventory
provider:
  name: aws
functions:
  inventory:
    handler: runtime.inventory
    events:
      - http: GET inventory/{sku}
      - http:
          method: post
          path: inventory
      - httpApi:
          method: '*'
          path: /inventory/any/{sku}
      - httpApi: '*'
      - http:
          path: 'GET /invented'
      - httpApi:
          method: '${env:METHOD}'
          path: /unknown
      - httpApi:
          method: GET
          path: '${env:PATH}'
"#,
    )
    .unwrap();
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::ANY::/inventory/any/{}", "site": "example.com/route-oracle/serverless::runtime.ts::inventory"},
            {"contract": "http::GET::/inventory/{}", "site": "example.com/route-oracle/serverless::runtime.ts::inventory"},
            {"contract": "http::POST::/inventory", "site": "example.com/route-oracle/serverless::runtime.ts::inventory"}
        ])
    );
    std::fs::remove_file(repo.join("runtime.ts")).unwrap();
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::ANY::/inventory/any/{}", "site": "example.com/route-oracle/serverless"},
            {"contract": "http::GET::/inventory/{}", "site": "example.com/route-oracle/serverless"},
            {"contract": "http::POST::/inventory", "site": "example.com/route-oracle/serverless"}
        ])
    );
}

#[test]
fn terraform_http_api_follows_only_its_lambda_integration() {
    let repo = repository("iac_http_terraform", "iac-http-terraform", "terraform");
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::GET::/orbits/{}", "site": "example.com/route-oracle/terraform::runtime/index.ts::handleOrbits"}
        ])
    );
}

#[test]
fn native_json_rest_and_adjacent_cases_match_the_prepared_whole_provider_sets() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/iac-http-cases");
    let cases: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("cases.json")).unwrap()).unwrap();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let fixture = case["fixture"].as_str().unwrap();
        let repo = repository(
            &format!("iac_http_case_{name}"),
            &format!("iac-http-{fixture}"),
            fixture,
        );
        for original in ["main.tf", "template.yaml", "serverless.yml"] {
            let _ = std::fs::remove_file(repo.join(original));
        }
        for source in std::fs::read_dir(root.join(name)).unwrap() {
            let source = source.unwrap().path();
            if source.is_file() {
                std::fs::copy(&source, repo.join(source.file_name().unwrap())).unwrap();
            }
        }
        assert_eq!(providers(&repo), case["providers"], "case {name}");
    }
}

#[test]
fn handler_aliases_and_unsafe_or_unknown_source_inputs_do_not_fabricate_a_handler() {
    let repo = repository("iac_http_handler_guard", "iac-http-sam", "sam");
    std::fs::write(
        repo.join("runtime/app.ts"),
        "function localOrchard() { return {}; }\nexport { localOrchard as orchard };\n",
    )
    .unwrap();
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::GET::/orchards/{}", "site": "example.com/route-oracle/sam::runtime/app.ts::localOrchard"},
            {"contract": "http::POST::/orchards", "site": "example.com/route-oracle/sam::runtime/app.ts::localOrchard"}
        ])
    );
    let source = std::fs::read_to_string(repo.join("template.yaml")).unwrap();
    std::fs::write(repo.join("bundle.zip"), "opaque bundle, not source").unwrap();
    for code_uri in [
        "bundle.zip",
        "../runtime",
        "/runtime",
        "s3://bucket/source.zip",
        "missing/",
    ] {
        std::fs::write(
            repo.join("template.yaml"),
            source.replace("runtime/", code_uri),
        )
        .unwrap();
        assert_eq!(
            providers(&repo),
            json!([
                {"contract": "http::GET::/orchards/{}", "site": "example.com/route-oracle/sam"},
                {"contract": "http::POST::/orchards", "site": "example.com/route-oracle/sam"}
            ]),
            "{code_uri}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("runtime", repo.join("linked")).unwrap();
        std::fs::write(
            repo.join("template.yaml"),
            source.replace("runtime/", "linked/"),
        )
        .unwrap();
        assert_eq!(providers(&repo)[0]["site"], "example.com/route-oracle/sam");
    }
    std::fs::write(
        repo.join("template.yaml"),
        source.replace(
            "Method: get",
            "Method: get\n            RestApiId: !Ref MissingApi",
        ),
    )
    .unwrap();
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::POST::/orchards", "site": "example.com/route-oracle/sam::runtime/app.ts::localOrchard"}
        ])
    );
}

#[test]
fn unknown_terraform_instances_never_choose_a_function() {
    let repo = repository(
        "iac_http_unknown_instance",
        "iac-http-terraform",
        "terraform",
    );
    let source = std::fs::read_to_string(repo.join("main.tf")).unwrap();
    std::fs::write(
        repo.join("main.tf"),
        source.replace(
            "resource \"aws_lambda_function\" \"orbits\" {",
            "resource \"aws_lambda_function\" \"orbits\" {\n  for_each = var.unread_functions",
        ),
    )
    .unwrap();
    assert_eq!(providers(&repo), json!([]));
}

#[test]
fn sam_global_handler_resolves_and_explicit_unknown_override_stays_at_the_repository() {
    let repo = repository("iac_http_global_handler", "iac-http-sam", "sam");
    let source = std::fs::read_to_string(repo.join("template.yaml")).unwrap();
    let global = source
        .replace(
            "Resources:",
            "Globals:\n  Function:\n    Handler: app.orchard\nResources:",
        )
        .replace("      Handler: app.orchard\n", "");
    std::fs::write(repo.join("template.yaml"), &global).unwrap();
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::GET::/orchards/{}", "site": "example.com/route-oracle/sam::runtime/app.ts::orchard"},
            {"contract": "http::POST::/orchards", "site": "example.com/route-oracle/sam::runtime/app.ts::orchard"}
        ])
    );
    std::fs::write(
        repo.join("template.yaml"),
        global.replace(
            "      Runtime:",
            "      Handler: !Ref UnreadHandler\n      Runtime:",
        ),
    )
    .unwrap();
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::GET::/orchards/{}", "site": "example.com/route-oracle/sam"},
            {"contract": "http::POST::/orchards", "site": "example.com/route-oracle/sam"}
        ])
    );
}

fn whole_workspace(case: &str) -> Vec<std::path::PathBuf> {
    let names = ["sam", "serverless", "terraform", "consumer"];
    let repos: Vec<_> = names
        .iter()
        .map(|name| repository(case, &format!("iac-http-{name}"), name))
        .collect();
    for (repo, name) in repos.iter().zip(names) {
        let mut config = std::fs::read_to_string(repo.join("meada.toml")).unwrap_or_default();
        if name == "consumer" {
            config.clear();
        }
        for peer in names.into_iter().filter(|peer| *peer != name) {
            config += &format!("\n[[peer]]\nrepo = \"example.com/route-oracle/{peer}\"\n");
        }
        std::fs::write(repo.join("meada.toml"), config).unwrap();
    }
    repos
}

#[test]
fn an_unread_serverless_method_never_serves_delete_or_other_consumers() {
    let repos = whole_workspace("iac_http_unread_method_link");
    let source = std::fs::read_to_string(repos[1].join("serverless.yml")).unwrap();
    std::fs::write(
        repos[1].join("serverless.yml"),
        source.replace("method: PATCH", "method: {Fn::If: [IsGet, GET, POST]}"),
    )
    .unwrap();
    let source = std::fs::read_to_string(repos[3].join("client.ts")).unwrap();
    std::fs::write(
        repos[3].join("client.ts"),
        source + "\nexport function removeInventory(sku: string) { return fetch(`https://inventory.example.test/inventory/${sku}`, { method: 'DELETE' }); }\n",
    )
    .unwrap();
    assert_eq!(providers(&repos[1]), json!([]));
    let exported = Engine::open(&repos[3]).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    let links = data["links"].as_array().unwrap();
    let expected = [
        (
            "createOrchard",
            "http::POST::/orchards",
            "sam::runtime/app.ts::orchard",
        ),
        (
            "readOrbit",
            "http::GET::/orbits/{}",
            "terraform::runtime/index.ts::handleOrbits",
        ),
        (
            "readOrchard",
            "http::GET::/orchards/{}",
            "sam::runtime/app.ts::orchard",
        ),
    ];
    assert_eq!(links.len(), expected.len(), "{exported}");
    for (link, (caller, contract, target)) in links.iter().zip(expected) {
        assert_eq!(
            link["consumer"],
            format!("example.com/route-oracle/consumer::client.ts::{caller}::fetch#0")
        );
        assert_eq!(link["contract"], contract);
        assert_eq!(
            link["provider"],
            format!("example.com/route-oracle/{target}")
        );
        assert_eq!(link["level"], "exact");
    }
}

#[test]
fn all_declared_routes_link_across_repositories_and_queries_match_every_row() {
    let repos = whole_workspace("iac_http_whole");
    let consumer = Engine::open(&repos[3]).unwrap();
    let exported = consumer.export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    let links = data["links"].as_array().unwrap();
    let expected = [
        (
            "createOrchard",
            "http::POST::/orchards",
            "sam::runtime/app.ts::orchard",
        ),
        (
            "patchInventory",
            "http::PATCH::/inventory/{}",
            "serverless::runtime.ts::inventory",
        ),
        (
            "readOrbit",
            "http::GET::/orbits/{}",
            "terraform::runtime/index.ts::handleOrbits",
        ),
        (
            "readOrchard",
            "http::GET::/orchards/{}",
            "sam::runtime/app.ts::orchard",
        ),
    ];
    assert_eq!(links.len(), expected.len(), "{exported}");
    for (link, (caller, contract, handler)) in links.iter().zip(expected) {
        assert_eq!(
            link["consumer"],
            format!("example.com/route-oracle/consumer::client.ts::{caller}::fetch#0")
        );
        assert_eq!(link["contract"], contract);
        assert_eq!(
            link["provider"],
            format!("example.com/route-oracle/{handler}")
        );
        assert_eq!(link["level"], "exact");
    }
    for (repo, name) in repos
        .iter()
        .zip(["sam", "serverless", "terraform", "consumer"])
    {
        let engine = Engine::open(repo).unwrap();
        assert_golden(
            &format!("iac-http-{name}.export.json"),
            &engine.export().unwrap(),
        );
    }
    let context = || consumer.context("readOrbit", Some(100_000)).unwrap();
    assert_eq!(context(), with_every_row(context));
    let trace = || {
        consumer
            .trace(Start::From("readOrbit"), Direction::Down, Some(100_000))
            .unwrap()
    };
    assert_eq!(trace(), with_every_row(trace));
    let search = || {
        consumer
            .search("orbits", Some("route"), Some(100_000))
            .unwrap()
    };
    assert_eq!(search(), with_every_row(search));
    assert_golden("iac-http-consumer.context.txt", &context());
    assert_golden("iac-http-consumer.trace.txt", &trace());
    assert_golden("iac-http-consumer.search.txt", &search());
    // Aliases use the same stored export projection in answer-sized and full reads.
    std::fs::write(
        repos[2].join("runtime/index.ts"),
        "function localOrbit() { return {}; }\nexport { localOrbit as handleOrbits };\n",
    )
    .unwrap();
    let engine = Engine::open(&repos[3]).unwrap();
    let query = || {
        engine
            .trace(Start::From("readOrbit"), Direction::Down, Some(100_000))
            .unwrap()
    };
    assert_eq!(query(), with_every_row(query));
    assert!(
        query().contains("localOrbit") && query().contains("runtime/index.ts"),
        "{}",
        query()
    );
    let aliased: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(aliased["links"].as_array().unwrap().len(), 4);
    assert_eq!(
        aliased["links"][2]["provider"],
        "example.com/route-oracle/terraform::runtime/index.ts::localOrbit"
    );
    std::fs::remove_file(repos[1].join("runtime.ts")).unwrap();
    let exported = Engine::open(&repos[3]).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(data["links"].as_array().unwrap().len(), 4, "{exported}");
    assert_eq!(
        data["links"][1]["provider"],
        "example.com/route-oracle/serverless"
    );
    // The target repository must export its incoming fallback link too.
    let exported = Engine::open(&repos[1]).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(data["links"].as_array().unwrap().len(), 1, "{exported}");
    assert_eq!(
        data["links"][0]["provider"],
        "example.com/route-oracle/serverless"
    );
}

#[test]
fn an_overridable_instance_never_upgrades_a_route_to_exact() {
    for (name, target) in [("route", false), ("lambda", true)] {
        let repos = whole_workspace(&format!("iac_http_overridable_{name}"));
        let path = repos[2].join("main.tf");
        let source = std::fs::read_to_string(&path).unwrap();
        let source = if target {
            source
                .replace(
                    "resource \"aws_lambda_function\" \"orbits\" {",
                    "resource \"aws_lambda_function\" \"orbits\" {\n count = var.instances",
                )
                .replace(
                    "aws_lambda_function.orbits.invoke_arn",
                    "aws_lambda_function.orbits[0].invoke_arn",
                )
        } else {
            source.replace(
                "resource \"aws_apigatewayv2_route\" \"read\" {",
                "resource \"aws_apigatewayv2_route\" \"read\" {\n count = var.instances",
            )
        };
        std::fs::write(&path, source + "\nvariable \"instances\" { default = 1 }\n").unwrap();
        let text = Engine::open(&repos[3]).unwrap().export().unwrap();
        let data: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(data["links"].as_array().unwrap().len(), 4, "{text}");
        assert_eq!(data["links"][2]["level"], "inferred", "{name}: {text}");
    }
}

#[test]
fn two_declared_handlers_keep_all_ambiguous_provider_candidates() {
    let sam = repository("iac_http_two_handlers", "iac-http-sam", "sam");
    let consumer = repository("iac_http_two_handlers", "iac-http-consumer", "consumer");
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/iac-http-cases/sam-two-handlers/template.yaml"),
    )
    .unwrap();
    std::fs::write(sam.join("template.yaml"), source).unwrap();
    std::fs::write(
        sam.join("runtime/other.ts"),
        "export function orchard() { return {}; }\n",
    )
    .unwrap();
    assert_eq!(
        providers(&sam),
        json!([
            {"contract": "http::GET::/orchards/{}", "site": "example.com/route-oracle/sam::runtime/app.ts::orchard"},
            {"contract": "http::GET::/orchards/{}", "site": "example.com/route-oracle/sam::runtime/other.ts::orchard"}
        ])
    );
    let text = Engine::open(&consumer).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(data["links"].as_array().unwrap().len(), 2, "{text}");
    for link in data["links"].as_array().unwrap() {
        assert_eq!(link["level"], "ambiguous", "{text}");
        assert_eq!(link["contract"], "http::GET::/orchards/{}");
    }
}

#[test]
fn terraform_module_outputs_locals_and_instance_keys_keep_the_exact_api_and_function() {
    let repo = repository(
        "iac_http_module_identity",
        "iac-http-terraform",
        "terraform",
    );
    let original = std::fs::read_to_string(repo.join("main.tf")).unwrap();
    let child = original
        .split("resource \"aws_apigatewayv2_route\"")
        .next()
        .unwrap()
        .replace(
            "handler       = \"index.handleOrbits\"",
            "handler       = var.handler",
        );
    assert!(child.contains("handler       = var.handler"));
    std::fs::create_dir_all(repo.join("modules/http")).unwrap();
    std::fs::write(
        repo.join("modules/http/main.tf"),
        format!(
            r#"
variable "handler" {{}}
{child}
output "api" {{ value = aws_apigatewayv2_api.public.id }}
output "integration" {{ value = aws_apigatewayv2_integration.orbits.id }}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        repo.join("runtime/other.ts"),
        "export function handleOther() { return {}; }\n",
    )
    .unwrap();
    let root = r#"
module "gateway" {
 source = "./modules/http"
 for_each = { east = "other.handleOther", west = "index.handleOrbits" }
 handler = each.value
}
locals {
 selected_api = module.gateway["west"].api
 selected_target = "integrations/${module.gateway["west"].integration}"
}
resource "aws_apigatewayv2_route" "read" {
 api_id = local.selected_api
 route_key = "GET /orbits/{orbitId}"
 target = local.selected_target
}
"#;
    std::fs::write(repo.join("main.tf"), root).unwrap();
    assert_eq!(
        providers(&repo),
        json!([
            {"contract": "http::GET::/orbits/{}", "site": "example.com/route-oracle/terraform::runtime/index.ts::handleOrbits"}
        ])
    );
    // A different instance owns a different API, even though the declarations have one path.
    std::fs::write(
        repo.join("main.tf"),
        root.replace(
            "selected_api = module.gateway[\"west\"]",
            "selected_api = module.gateway[\"east\"]",
        ),
    )
    .unwrap();
    assert_eq!(providers(&repo), json!([]));
    std::fs::write(
        repo.join("main.tf"),
        root.replace("module.gateway[\"west\"]", "module.gateway[var.unread_key]"),
    )
    .unwrap();
    assert_eq!(providers(&repo), json!([]));
}
