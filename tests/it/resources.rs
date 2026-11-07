//! Resource queries (spec #19) over the chain fixture: the gateway writes the `postgres`
//! resource `orders` through `cancelOrder`, the worker reads it through `settleOrders` and the
//! catalog reads it as a whole repository.

use crate::common;

use common::{assert_golden, chain, loose_chain};
use meada::engine::{self, Direction, Engine, Start};

#[test]
fn search_kind_resource_finds_a_resource_by_its_id_and_by_its_kind() {
    let (web, _, _) = chain("resource_search");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "chain.search-resource.txt",
        &engine.search("orders", Some("resource"), None).unwrap(),
    );
    // The kind matches too, and `route` never lists a resource.
    let by_kind = engine.search("postgres", Some("resource"), None).unwrap();
    assert_eq!(by_kind.lines().count(), 3, "{by_kind}");
    let routes = engine.search("orders", Some("route"), None).unwrap();
    assert!(!routes.contains("resource::"), "{routes}");
    let all = engine.search("orders", Some("contract"), None).unwrap();
    assert!(all.contains("resource::postgres::orders"), "{all}");
}

#[test]
fn context_on_a_resource_lists_its_writers_and_readers() {
    let (web, _, _) = chain("resource_context");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "chain.context-resource.txt",
        &engine.context("resource::postgres::orders", None).unwrap(),
    );
    assert_golden(
        "chain.context-resource-none.txt",
        &engine.context("resource::postgres::gone", None).unwrap(),
    );
}

#[test]
fn context_on_a_reader_symbol_shows_the_resource_like_a_writer_does() {
    let (web, _, _) = chain("resource_context_reader");
    let engine = Engine::open(&web).unwrap();

    let reader = engine.context("settleOrders", None).unwrap();
    let writer = engine
        .context(
            "example.com/chain/gateway::src/orders.ts::cancelOrder",
            None,
        )
        .unwrap();

    assert!(
        reader.contains("consumes resource::postgres::orders"),
        "{reader}"
    );
    assert!(
        writer.contains("provides resource::postgres::orders"),
        "{writer}"
    );
}

#[test]
fn trace_does_not_start_from_a_resource_and_points_to_context() {
    let (web, _, _) = chain("resource_trace_start");

    let answer = Engine::open(&web)
        .unwrap()
        .trace(
            Start::From("resource::postgres::orders"),
            Direction::Down,
            None,
        )
        .unwrap();

    assert_golden("chain.trace-resource-start.txt", &answer);
}

#[test]
fn status_reports_a_resource_with_one_side_and_a_via_that_names_no_symbol() {
    let gateway = loose_chain("resource_status");
    Engine::open(&gateway).unwrap().refresh().unwrap();

    assert_golden("chain-loose.status.txt", &engine::status(&gateway).unwrap());
}

fn via_workspace(case: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        common::fixture_repo(
            case,
            "resource-via-infra",
            Some("https://example.com/resource-via/infra.git"),
        ),
        common::fixture_repo(
            case,
            "resource-via-app",
            Some("https://example.com/resource-via/app.git"),
        ),
    )
}

#[test]
fn a_resource_via_names_an_iac_node_and_keeps_its_side_declared() {
    let (infra, _) = via_workspace("resource_via_iac");
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["consumers"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(
        json["consumers"][0]["site"],
        "example.com/resource-via/infra::main.tf::opsgenie_team_routing_rule.team_b"
    );
    assert_eq!(json["links"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(json["links"][0]["level"], "declared");
    assert!(
        !engine::status(&infra)
            .unwrap()
            .contains("matches no symbol")
    );
}

#[test]
fn resource_key_path_selectors_anchor_the_writer_and_trace_to_its_reader() {
    let (infra, app) = via_workspace("resource_via_key");
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["providers"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(
        json["providers"][0]["site"],
        "example.com/resource-via/app::values.yaml#groups[1].rules[0].labels.support"
    );
    let mut export = meada_core::export::parse(&text).unwrap();
    export.commit = None;
    export.dirty = None;
    assert_golden("resource-via-app.export.json", &export.write().unwrap());
    let selector = "values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support";
    let key = engine.context(selector, Some(12000)).unwrap();
    assert!(
        key.contains("provides resource::routing::team-b")
            && key.contains("opsgenie_team_routing_rule.team_b"),
        "{key}"
    );
    let node = engine
        .context("opsgenie_team_routing_rule.team_b", Some(12000))
        .unwrap();
    assert!(
        node.contains("consumes resource::routing::team-b")
            && node.contains("values.yaml#groups[1].rules[0].labels.support"),
        "{node}"
    );
    let contract = engine
        .context("resource::routing::team-b", Some(12000))
        .unwrap();
    assert!(!contract.contains("repository-level"), "{contract}");
    assert!(
        contract.contains("values.yaml#groups[1].rules[0].labels.support (line 11)"),
        "{contract}"
    );
    assert_golden("resource-via.context.txt", &(key + &node + &contract));
    let down = engine
        .trace(Start::From(selector), Direction::Down, Some(12000))
        .unwrap();
    assert!(
        down.contains("opsgenie_team_routing_rule.team_b") && down.contains("declared"),
        "{down}"
    );
    let up = Engine::open(&infra)
        .unwrap()
        .trace(
            Start::From("opsgenie_team_routing_rule.team_b"),
            Direction::Up,
            Some(12000),
        )
        .unwrap();
    assert!(
        up.contains("values.yaml#groups[1].rules[0].labels.support") && up.contains("declared"),
        "{up}"
    );
    assert_golden("resource-via.trace.txt", &(down + &up));
}

#[test]
fn resource_via_addresses_cover_every_format_and_qualify_templates() {
    let (infra, _) = via_workspace("resource_via_formats");
    let addresses = [
        (
            "opsgenie_team_routing_rule.team_b",
            "main.tf::opsgenie_team_routing_rule.team_b",
        ),
        (
            "custom_rule.hashed[\"route#b\"]",
            "main.tf::custom_rule.hashed[\"route#b\"]",
        ),
        ("custom_rule.open", "open.tofu::custom_rule.open"),
        (
            "ConfigMap/shop/routing",
            "kube.yaml::ConfigMap/shop/routing",
        ),
        (
            "Bucket.s3.aws.upbound.io/archive",
            "crossplane.yaml::Bucket.s3.aws.upbound.io/archive",
        ),
        ("services.router", "compose.yaml::services.router"),
        ("stack.yaml::Routing", "stack.yaml::Routing"),
        ("sam.yaml::Routing", "sam.yaml::Routing"),
        (
            "serverless.yml::functions.route",
            "serverless.yml::functions.route",
        ),
    ];
    let vias: Vec<_> = addresses
        .iter()
        .map(|(via, _)| *via)
        .chain(["stack.yaml::Gone", "custom_rule.open (root .)", "Routing"])
        .collect();
    std::fs::write(infra.join("meada.toml"), format!(
        "[[peer]]\nrepo = \"example.com/resource-via/app\"\n\n[[resource]]\nkind = \"routing\"\nid = \"team-b\"\nrole = \"read-write\"\nvia = {}\n", serde_json::to_string(&vias).unwrap()
    )).unwrap();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut wanted: Vec<_> = addresses
        .iter()
        .map(|(_, id)| format!("example.com/resource-via/infra::{id}"))
        .collect();
    wanted.sort();
    let actual: Vec<_> = json["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["site"].as_str().unwrap())
        .collect();
    assert_eq!(actual, wanted, "{text}");
    let providers: Vec<_> = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["site"].as_str().unwrap())
        .collect();
    assert_eq!(providers, wanted, "{text}");
    let nodes: Vec<_> = json["iac_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["identity"].as_str().unwrap())
        .collect();
    assert_eq!(nodes, wanted, "{text}");
    assert_eq!(
        json["links"].as_array().unwrap().len(),
        wanted.len(),
        "{text}"
    );
    assert!(
        json["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["level"] == "declared"),
        "{text}"
    );
    let mut export = meada_core::export::parse(&text).unwrap();
    export.commit = None;
    export.dirty = None;
    assert_golden("resource-via-infra.export.json", &export.write().unwrap());
    let status = engine::status(&infra).unwrap();
    let stale: Vec<_> = status
        .lines()
        .filter(|line| line.contains("`via`"))
        .collect();
    assert_eq!(stale.len(), 3, "{status}");
    assert!(
        stale
            .iter()
            .any(|l| l.contains("stack.yaml::Gone matches no symbol, ignored")),
        "{status}"
    );
    assert!(
        stale
            .iter()
            .any(|l| l.contains("custom_rule.open (root .) matches no symbol, ignored")),
        "{status}"
    );
    assert!(
        stale
            .iter()
            .any(|l| l.contains("`via` Routing matches no symbol, ignored")),
        "{status}"
    );
    assert_golden("resource-via.status.txt", &status);
    for (via, _) in addresses {
        let context = engine.context(via, Some(12000)).unwrap();
        assert!(
            context.contains("consumes resource::routing::team-b"),
            "{via}: {context}"
        );
        let up = engine
            .trace(Start::From(via), Direction::Up, Some(12000))
            .unwrap();
        assert!(
            up.contains("values.yaml#groups[1].rules[0].labels.support"),
            "{via}: {up}"
        );
    }
}

#[test]
fn resource_key_selectors_keep_quoted_paths_documents_and_secret_boundaries() {
    let (infra, app) = via_workspace("resource_via_selectors");
    let needle = "ops]\"=team";
    std::fs::write(app.join("selectors.json"), serde_json::to_string_pretty(&serde_json::json!({
        "monitors": [
            {"name": needle, "metadata": {"owner": "team-b"}, "labels": {"support/team": "team-b"}},
            {"name": "other", "metadata": {"owner": "team-a"}, "labels": {"support/team": "team-a"}},
            {"name": needle, "metadata": {"owner": "team-b"}, "labels": {"support/team": "team-b"}}
        ],
        "literal.key": [{"target": "team-b"}],
        "null_value": null,
        "empty_object": {},
        "empty_list": [],
        "records": [{"key": "x".repeat(1025), "target": "team-b"}]
    })).unwrap()).unwrap();
    std::fs::write(
        app.join("bundle.yaml"),
        "labels:\n  support: other\n---\nlabels:\n  support: team-b\n",
    )
    .unwrap();
    std::fs::write(app.join("secrets.yaml"), "apiVersion: v1\nkind: List\nitems:\n  - apiVersion: v1\n    kind: Secret\n    metadata:\n      name: credentials\n    stringData:\n      password: private-selector-value\n").unwrap();
    std::fs::write(app.join("ignored.yaml"), "labels:\n  support: team-b\n").unwrap();
    std::fs::write(app.join(".meadaignore"), "ignored.yaml\n").unwrap();
    let selector = format!(
        "selectors.json#monitors[name={}].labels[\"support/team\"]",
        serde_json::to_string(needle).unwrap()
    );
    let bad = [
        "../resource-via-infra/kube.yaml#data.support".to_string(),
        format!("{}#data.support", infra.join("kube.yaml").display()),
        "ignored.yaml#labels.support".into(),
        "selectors.json#monitors[nope].labels".into(),
        "selectors.json#monitors[name=absent].labels".into(),
        "selectors.json#null_value".into(),
        "selectors.json#empty_object".into(),
        "selectors.json#empty_list".into(),
        "bundle.yaml#[5].labels.support".into(),
        "secrets.yaml#items[stringData.password=private-selector-value].metadata.name".into(),
        format!("selectors.json#records[key={}].target", "x".repeat(1024)),
    ];
    let mut vias = vec![
        selector.clone(),
        "selectors.json#monitors[metadata.owner=team-b].labels[\"support/team\"]".into(),
        "selectors.json#[\"literal.key\"][0].target".into(),
        "bundle.yaml#[1].labels.support".into(),
        "secrets.yaml#items[0].stringData.password".into(),
    ];
    vias.extend(bad.iter().cloned());
    std::fs::write(app.join("meada.toml"), format!("[[peer]]\nrepo = \"example.com/resource-via/infra\"\n\n[[resource]]\nkind = \"routing\"\nid = \"team-b\"\nrole = \"write\"\nvia = {}\n", serde_json::to_string(&vias).unwrap())).unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let locations = [
        "bundle.yaml#[1].labels.support",
        "secrets.yaml#items[0].stringData.password",
        "selectors.json#[\"literal.key\"][0].target",
        "selectors.json#monitors[0].labels[\"support/team\"]",
        "selectors.json#monitors[2].labels[\"support/team\"]",
    ];
    let wanted: Vec<_> = locations
        .iter()
        .map(|key| format!("example.com/resource-via/app::{key}"))
        .collect();
    let actual: Vec<_> = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["site"].as_str().unwrap())
        .collect();
    assert_eq!(actual, wanted, "{text}");
    assert!(json["consumers"].as_array().unwrap().is_empty(), "{text}");
    assert_eq!(
        json["links"].as_array().unwrap().len(),
        locations.len(),
        "{text}"
    );
    assert!(
        !text.contains("private-selector-value") && !text.contains(&"x".repeat(1024)),
        "{text}"
    );
    let status = engine::status(&app).unwrap();
    let stale: Vec<_> = status
        .lines()
        .filter(|line| line.contains("`via`"))
        .collect();
    assert_eq!(stale.len(), bad.len(), "{status}");
    for via in &bad {
        assert!(
            stale.iter().any(|line| line.contains(via)),
            "{via}: {status}"
        );
    }
    let context = engine.context(&selector, Some(12000)).unwrap();
    assert_eq!(context.lines().count(), 6, "{context}");
    assert!(
        context.contains("monitors[0]")
            && context.contains("monitors[2]")
            && !context.contains("monitors[1]"),
        "{context}"
    );
    for key in locations {
        let trace = engine
            .trace(Start::From(key), Direction::Down, Some(12000))
            .unwrap();
        assert!(
            trace.contains("opsgenie_team_routing_rule.team_b"),
            "{key}: {trace}"
        );
    }
}

#[test]
fn resource_via_deleted_keys_and_nodes_are_stale_and_restoration_recovers_locations() {
    let (infra, app) = via_workspace("resource_via_stale");
    let app_engine = Engine::open(&app).unwrap();
    let infra_engine = Engine::open(&infra).unwrap();
    let canonical = |text: String| {
        let mut export = meada_core::export::parse(&text).unwrap();
        export.commit = None;
        export.dirty = None;
        export.write().unwrap()
    };
    let before = canonical(app_engine.export().unwrap());
    let values = std::fs::read_to_string(app.join("values.yaml")).unwrap();
    let terraform = std::fs::read_to_string(infra.join("main.tf")).unwrap();
    std::fs::write(
        app.join("values.yaml"),
        values.replace("availability", "unavailablex"),
    )
    .unwrap();
    std::fs::remove_file(infra.join("main.tf")).unwrap();
    for (engine, identity, role) in [
        (&app_engine, "example.com/resource-via/app", "providers"),
        (&infra_engine, "example.com/resource-via/infra", "consumers"),
    ] {
        let text = engine.export().unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json[role].as_array().unwrap().len(), 1, "{text}");
        assert_eq!(json[role][0]["site"], identity, "{text}");
    }
    let status = engine::status(&app).unwrap();
    assert_eq!(
        status.lines().filter(|l| l.contains("`via`")).count(),
        4,
        "{status}"
    );
    assert_golden("resource-via.stale-status.txt", &status);
    let old_key = "values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support";
    assert!(
        app_engine
            .context(old_key, None)
            .unwrap()
            .starts_with("no symbol matches")
    );
    assert!(
        app_engine
            .trace(
                Start::From("opsgenie_team_routing_rule.team_b"),
                Direction::Up,
                None
            )
            .unwrap()
            .starts_with("no symbol, route or call site matches")
    );
    std::fs::write(app.join("values.yaml"), values).unwrap();
    std::fs::write(infra.join("main.tf"), terraform).unwrap();
    assert_eq!(canonical(app_engine.export().unwrap()), before);
    assert!(!engine::status(&app).unwrap().contains("matches no symbol"));
}

#[test]
fn resource_key_sites_use_server_indexes_and_only_the_manifest_scope() {
    let (infra, app) = via_workspace("resource_via_server");
    let folder = app.parent().unwrap();
    let manifest = folder.join("workspace.toml");
    std::fs::write(&manifest, format!("index_dir = \"indexes\"\n\n[[repository]]\nrepo = \"example.com/resource-via/app\"\npath = {:?}\n\n[[repository]]\nrepo = \"example.com/resource-via/infra\"\npath = {:?}\n", app.to_str().unwrap(), infra.to_str().unwrap())).unwrap();
    engine::index_workspace(&manifest).unwrap();
    let selector = "values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support";
    let before = Engine::open(&app)
        .unwrap()
        .context(selector, Some(12000))
        .unwrap();
    std::fs::remove_file(app.join("values.yaml")).unwrap();
    let server = Engine::open_manifest(&manifest, "example.com/resource-via/app").unwrap();
    assert_eq!(server.context(selector, Some(12000)).unwrap(), before);
    let narrow = folder.join("app-only.toml");
    std::fs::write(&narrow, format!("index_dir = \"indexes\"\n\n[[repository]]\nrepo = \"example.com/resource-via/app\"\npath = {:?}\n", app.to_str().unwrap())).unwrap();
    let hidden = Engine::open_manifest(&narrow, "example.com/resource-via/app").unwrap();
    let context = hidden.context(selector, Some(12000)).unwrap();
    assert!(
        context.contains("provides resource::routing::team-b")
            && !context.contains("resource-via/infra"),
        "{context}"
    );
    let trace = hidden
        .trace(Start::From(selector), Direction::Down, Some(12000))
        .unwrap();
    assert!(
        !trace.contains("opsgenie_team_routing_rule.team_b"),
        "{trace}"
    );
}

#[test]
fn declared_keys_keep_automatic_uses_and_continue_through_read_write_sides() {
    let (middle, first) = via_workspace("resource_via_continuation");
    let last = first.parent().unwrap().join("last");
    if last.exists() {
        std::fs::remove_dir_all(&last).unwrap();
    }
    std::fs::create_dir_all(&last).unwrap();
    common::git(&last, &["init", "-q"]);
    common::git(
        &last,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/resource-via/last.git",
        ],
    );
    for (repo, role, peers) in [
        (&first, "write", vec!["infra", "last"]),
        (&middle, "read-write", vec!["app", "last"]),
        (&last, "read", vec!["app", "infra"]),
    ] {
        std::fs::write(repo.join("use.yaml"), "queue: orders\n").unwrap();
        let peers = peers
            .into_iter()
            .map(|peer| format!("[[peer]]\nrepo = \"example.com/resource-via/{peer}\"\n"))
            .collect::<String>();
        std::fs::write(repo.join("meada.toml"), format!("{peers}\n[[resource]]\nkind = \"routing\"\nid = \"team-b\"\nrole = \"{role}\"\nvia = [\"use.yaml#queue\"]\n")).unwrap();
    }
    let engine = Engine::open(&first).unwrap();
    let context = engine
        .context(
            "example.com/resource-via/infra::use.yaml#queue",
            Some(12000),
        )
        .unwrap();
    assert!(
        context.contains("use use.yaml#queue (line 1) [inferred] channel::sqs::orders"),
        "{context}"
    );
    assert!(
        context.contains("provides resource::routing::team-b")
            && context.contains("consumes resource::routing::team-b"),
        "{context}"
    );
    let trace = engine
        .trace(
            Start::From("example.com/resource-via/app::use.yaml#queue"),
            Direction::Down,
            Some(12000),
        )
        .unwrap();
    assert!(
        trace.lines().any(|line| line.starts_with("    →[declared:")
            && line.contains("example.com/resource-via/last use.yaml#queue")),
        "{trace}"
    );
}

#[test]
fn unqualified_declared_key_queries_keep_inferred_matches_in_other_repositories() {
    let (infra, app) = via_workspace("resource_via_union");
    for repo in [&app, &infra] {
        std::fs::write(repo.join("use.yaml"), "queue: orders\n").unwrap();
    }
    std::fs::write(app.join("meada.toml"), "[[peer]]\nrepo = \"example.com/resource-via/infra\"\n\n[[resource]]\nkind = \"routing\"\nid = \"team-b\"\nrole = \"write\"\nvia = [\"use.yaml#queue\"]\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    let context = engine.context("use.yaml#queue", Some(12000)).unwrap();
    let uses: Vec<_> = context
        .lines()
        .filter(|line| line.contains("use use.yaml#queue (line 1) [inferred] channel::sqs::orders"))
        .collect();
    assert_eq!(uses.len(), 2, "{context}");
    assert!(
        uses.iter()
            .any(|line| line.starts_with("example.com/resource-via/app "))
            && uses
                .iter()
                .any(|line| line.starts_with("example.com/resource-via/infra ")),
        "{context}"
    );
    for direction in [Direction::Down, Direction::Up] {
        let trace = engine
            .trace(Start::From("use.yaml#queue"), direction, Some(12000))
            .unwrap();
        assert_eq!(
            trace
                .lines()
                .filter(|line| line.starts_with("example.com/resource-via/"))
                .count(),
            2,
            "{trace}"
        );
        assert!(
            trace
                .lines()
                .any(|line| line.starts_with("example.com/resource-via/app use.yaml#queue")),
            "{trace}"
        );
        assert!(
            trace
                .lines()
                .any(|line| line.starts_with("example.com/resource-via/infra use use.yaml#queue")),
            "{trace}"
        );
    }
}

#[test]
fn selectors_do_not_equate_hcl_expressions_or_templates_with_literal_strings() {
    let (_, app) = via_workspace("resource_via_expression");
    std::fs::write(
        app.join("settings.tf"),
        r#"locals {
  targets = [
    { name = var.name, target = "team-b" },
    { name = "var.name", target = "team-b" },
    { name = "${var.name}", target = "team-b" },
    { name = "\"$${var.name}\"", target = "team-b" }
  ]
}
"#,
    )
    .unwrap();
    let selectors = [
        r#"settings.tf#locals.targets[name="var.name"].target"#,
        r#"settings.tf#locals.targets[name="\"${var.name}\""].target"#,
    ];
    std::fs::write(app.join("meada.toml"), format!("[[peer]]\nrepo = \"example.com/resource-via/infra\"\n\n[[resource]]\nkind = \"routing\"\nid = \"team-b\"\nrole = \"write\"\nvia = {}\n", serde_json::to_string(&selectors).unwrap())).unwrap();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    let sites: Vec<_> = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|side| side["site"].as_str().unwrap())
        .collect();
    assert_eq!(
        sites,
        [
            "example.com/resource-via/app::settings.tf#locals.targets[1].target",
            "example.com/resource-via/app::settings.tf#locals.targets[3].target"
        ],
        "{text}"
    );
}
