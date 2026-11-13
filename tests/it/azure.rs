//! Azure provisioning and Function workloads (#669, ADR 0026).
use crate::common::{assert_golden, fixture_repo};
use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

fn workspace(case: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let infra = fixture_repo(
        case,
        "azure-infra",
        Some("https://example.com/azure/infra.git"),
    );
    let app = fixture_repo(case, "azure-app", Some("https://example.com/azure/app.git"));
    (infra, app)
}

#[test]
fn azure_identifiers_provision_their_exact_kind_and_typed_keys_stay_inferred() {
    let (infra, app) = workspace("azure_kinds");
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let uses = json["uses"].as_array().expect("Azure config uses");
    assert_eq!(uses.len(), 16, "{text}");
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 16, "{text}");
    for link in provisions {
        let typed = link["at"].as_str().unwrap().split('#').nth(1).unwrap();
        assert_eq!(
            link["level"],
            if typed.starts_with("servicebus_")
                || typed.starts_with("eventhubs_")
                || typed.starts_with("azure")
                || typed.starts_with("keyvault_")
            {
                "inferred"
            } else {
                "exact"
            },
            "{link}"
        );
    }
    let provisioned: Value =
        serde_json::from_str(&Engine::open(&infra).unwrap().export().unwrap()).unwrap();
    let mut contracts = provisioned["provisioners"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    contracts.sort();
    assert_eq!(
        contracts,
        [
            "channel::eventhubs::events",
            "channel::servicebus::events",
            "channel::servicebus::orders",
            "channel::servicebus::orders/subscriptions/audit",
            "channel::servicebus::subscription:events/audit",
            "resource::azureblob::assetsstore",
            "resource::azurestorage::assetsstore",
            "resource::keyvault::api-key"
        ]
    );
    assert!(!text.contains("not-exported-secret-content"));
    assert_golden("azure.config.export.json", &text);
    let context = engine
        .context("settings.yaml#subscription_id", Some(8000))
        .unwrap();
    assert!(
        context.contains("channel::servicebus::subscription:events/audit"),
        "{context}"
    );
    let up = engine
        .trace(
            Start::From("azurerm_servicebus_subscription.audit"),
            Direction::Down,
            Some(12000),
        )
        .unwrap();
    assert!(up.contains("settings.yaml#subscription_id"), "{up}");
    let down = engine
        .trace(
            Start::From("settings.yaml#subscription_id"),
            Direction::Up,
            Some(12000),
        )
        .unwrap();
    assert!(
        down.contains("azurerm_servicebus_subscription.audit"),
        "{down}"
    );
    assert_golden("azure.config.context.txt", &context);
    assert_golden("azure.config.trace.txt", &format!("{down}\n{up}"));
    let (_, reports) = engine.status_report().unwrap();
    let report = reports.into_iter().flatten().collect::<Vec<_>>().join("\n");
    assert!(
        report.contains("orphan channel::servicebus::orders/subscriptions/audit: no use"),
        "{report}"
    );
}

#[test]
fn arm_resource_groups_keep_valid_unicode_selectors_and_reject_invalid_ones() {
    let (_, app) = workspace("azure_resource_group_selectors");
    let arm = |group: &str| {
        format!(
            "/subscriptions/deployment/resourceGroups/{group}/providers/Microsoft.ServiceBus/namespaces/bus/queues/orders"
        )
    };
    let mut settings = String::new();
    for (key, group) in [
        ("normal", "rgplain".to_string()),
        ("parentheses", "rg(prod)".to_string()),
        ("unicode", "équipe_部署-١".to_string()),
        ("maximum", "界".repeat(90)),
        ("over_maximum", "界".repeat(91)),
        ("bad_group", "rg#prod".to_string()),
        ("bad_ending", "rg.".to_string()),
        ("bad_category", "rg²".to_string()),
        ("bad_combining", "rge\u{301}".to_string()),
    ] {
        settings.push_str(&format!(
            "{key}: {}\n",
            serde_json::to_string(&arm(&group)).unwrap()
        ));
    }
    settings.push_str(&format!(
        "bad_namespace: {}\nbad_entity: {}\n",
        serde_json::to_string(&arm("rg(prod)").replace("namespaces/bus/", "namespaces/ns:bad/"))
            .unwrap(),
        serde_json::to_string(&arm("rg(prod)").replace("queues/orders", "queues/or:ders")).unwrap()
    ));
    std::fs::write(app.join("settings.yaml"), settings).unwrap();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let uses = data["uses"].as_array().unwrap();
    let mut actual = uses
        .iter()
        .map(|item| {
            assert_eq!(item["contract"], "channel::servicebus::orders");
            item["at"].as_str().unwrap()
        })
        .collect::<Vec<_>>();
    actual.sort();
    assert_eq!(
        actual,
        [
            "settings.yaml#maximum",
            "settings.yaml#normal",
            "settings.yaml#parentheses",
            "settings.yaml#unicode"
        ],
        "{text}"
    );
    let provisions = data["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 4, "{text}");
    assert!(
        provisions.iter().all(|link| link["level"] == "exact"),
        "{text}"
    );
}

#[test]
fn servicebus_subscriptions_keep_topic_identity_filters_and_slash_queues_separate() {
    let (infra, app) = workspace("azure_routes");
    std::fs::write(
        infra.join("main.tf"),
        include_str!("../fixtures/azure-routes/main.tf"),
    )
    .unwrap();
    std::fs::remove_file(app.join("settings.yaml")).unwrap();
    std::fs::write(
        app.join("asyncapi.yaml"),
        include_str!("../fixtures/azure-routes/asyncapi.yaml"),
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 4, "{text}");
    for (target, source, level) in [
        ("subscription:orders/orders", "orders", "inferred"),
        ("subscription:orders/audit", "orders", "exact"),
        ("subscription:other/audit", "other", "exact"),
    ] {
        let link = links
            .iter()
            .find(|l| l["contract"] == format!("channel::servicebus::{target}"))
            .unwrap();
        assert_eq!(link["level"], level, "{link}");
        assert!(
            link["provider"]
                .as_str()
                .unwrap()
                .ends_with(&format!("send {source}")),
            "{link}"
        );
        if level == "inferred" {
            assert!(
                link["reason"].as_str().unwrap().contains("filter"),
                "{link}"
            );
        }
    }
    assert_eq!(
        links
            .iter()
            .filter(|l| l["contract"] == "channel::eventhubs::events")
            .count(),
        1
    );
    let context = engine
        .context(
            "channel::servicebus::subscription:orders/orders",
            Some(12000),
        )
        .unwrap();
    assert!(
        context.contains("azurerm_servicebus_subscription.same") && context.contains("filter"),
        "{context}"
    );
    assert_golden("azure.routes.export.json", &text);
    let path = infra.join("main.tf");
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        original.replace(
            "resource \"azurerm_servicebus_subscription_rule\" \"filter\"",
            "resource \"unknown_rule\" \"filter\"",
        ),
    )
    .unwrap();
    let unfiltered: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let same = unfiltered["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["contract"] == "channel::servicebus::subscription:orders/orders")
        .unwrap();
    assert_eq!(same["level"], "exact", "{same}");
    std::fs::write(
        &path,
        original.replace(
            "resource \"azurerm_servicebus_subscription\" \"same\"",
            "resource \"unknown_subscription\" \"same\"",
        ),
    )
    .unwrap();
    let missing: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(missing["links"].as_array().unwrap().len(), 3, "{missing}");
    assert!(
        missing["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["contract"] != "channel::servicebus::subscription:orders/orders")
    );
}

#[test]
fn existing_csharp_senders_and_legacy_topic_processors_join_azure_provisioners() {
    let (_, app) = workspace("azure_csharp");
    std::fs::remove_file(app.join("settings.yaml")).unwrap();
    std::fs::write(app.join("Bus.cs"), "using Azure.Messaging.ServiceBus;\nclass Bus {\n async void Send() { var bus = new ServiceBusClient(\"cs\"); await bus.CreateSender(\"orders\").SendMessageAsync(new ServiceBusMessage()); await bus.CreateSender(\"events\").SendMessageAsync(new ServiceBusMessage()); }\n void Listen() { var bus = new ServiceBusClient(\"cs\"); var queue = bus.CreateProcessor(\"orders\"); queue.ProcessMessageAsync += OnMessage; var topic = bus.CreateProcessor(\"events\", \"audit\"); topic.ProcessMessageAsync += OnMessage; }\n void OnMessage(ProcessMessageEventArgs e) {}\n}\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    assert!(
        links.iter().all(|l| l["level"] == "exact"
            && ["channel::servicebus::orders", "channel::servicebus::events"]
                .contains(&l["contract"].as_str().unwrap())),
        "{text}"
    );
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 4, "{text}");
    assert!(
        provisions
            .iter()
            .all(|p| p["level"] == "exact"
                && !p["contract"].as_str().unwrap().contains("subscription:")),
        "{text}"
    );
}

#[test]
fn functions_run_images_or_safe_local_archives_and_resolve_only_their_app_settings() {
    let infra = fixture_repo(
        "azure_functions",
        "azure-functions",
        Some("https://example.com/azure/infra.git"),
    );
    let app = fixture_repo(
        "azure_functions",
        "azure-function-app",
        Some("https://example.com/azure/app.git"),
    );
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let executions = json["executions"].as_array().unwrap();
    assert_eq!(executions.len(), 2, "{text}");
    assert_eq!(
        executions
            .iter()
            .filter(|x| x["repository"] == "example.com/azure/infra"
                && ["worker.zip", "local.zip"].contains(&x["path"].as_str().unwrap()))
            .count(),
        2,
        "{text}"
    );
    assert_eq!(json["links"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(
        json["links"][0]["contract"],
        "image::ghcr.io/azure/functions"
    );
    assert_eq!(json["links"][0]["level"], "inferred");
    let resolved: Value =
        serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    let provisions = resolved["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 2, "{resolved}");
    assert!(
        provisions
            .iter()
            .all(|p| p["contract"] == "channel::sqs::tasks"
                && p["level"] == "inferred"
                && p["reason"]
                    .as_str()
                    .unwrap()
                    .contains("azurerm_linux_function_app.image")),
        "{resolved}"
    );
    let unresolved = resolved["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["contract"] == "channel::sqs")
        .count();
    assert_eq!(unresolved, 2, "{resolved}");
    assert!(!text.contains("@Microsoft.KeyVault"));
    let context = engine
        .context("azurerm_linux_function_app.image", Some(12000))
        .unwrap();
    assert!(
        context.contains("env")
            && context.contains("SECRET_QUEUE")
            && context.contains("redacted")
            && context.contains("Key Vault secret api-key")
            && context.contains("ghcr.io/azure/functions"),
        "{context}"
    );
    let archive = engine
        .context("azurerm_windows_function_app.archive", Some(8000))
        .unwrap();
    assert!(
        archive.contains("worker.zip") && archive.contains("code:"),
        "{archive}"
    );
    let down = engine
        .trace(
            Start::From("azurerm_linux_function_app.image"),
            Direction::Down,
            Some(12000),
        )
        .unwrap();
    assert!(
        down.contains("app.ts") && down.contains("example.com/azure/app"),
        "{down}"
    );
    let up = Engine::open(&app)
        .unwrap()
        .trace(Start::From("task"), Direction::Up, Some(12000))
        .unwrap();
    assert!(up.contains("azurerm_linux_function_app.image"), "{up}");
    assert_golden("azure.functions.export.json", &text);
    assert_golden("azure.functions.context.txt", &(context + "\n" + &archive));
    assert_golden("azure.functions.trace.txt", &(down + "\n" + &up));
    let original = std::fs::read_to_string(infra.join("main.tf")).unwrap();
    std::fs::write(
        infra.join("main.tf"),
        original.replace(
            "zip_deploy_file = \"worker.zip\"",
            "zip_deploy_file = \"sub/../worker.zip\"",
        ),
    )
    .unwrap();
    let changed: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        changed["executions"].as_array().unwrap().len(),
        1,
        "{changed}"
    );
    std::fs::write(
        infra.join("main.tf"),
        original.replace(
            "zip_deploy_file = \"worker.zip\"",
            "zip_deploy_file = \".\"",
        ),
    )
    .unwrap();
    let directory: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        directory["executions"].as_array().unwrap().len(),
        1,
        "{directory}"
    );
    std::fs::remove_file(infra.join("local.zip")).unwrap();
    let changed: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(changed.get("executions").is_none(), "{changed}");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("worker.zip", infra.join("local.zip")).unwrap();
        let symlinked: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
        assert!(symlinked.get("executions").is_none(), "{symlinked}");
    }
}

#[test]
fn ambiguous_function_images_supply_no_environment_scope() {
    let infra = fixture_repo(
        "azure_ambiguous",
        "azure-functions",
        Some("https://example.com/azure/infra.git"),
    );
    let app = fixture_repo(
        "azure_ambiguous",
        "azure-function-app",
        Some("https://example.com/azure/app.git"),
    );
    fixture_repo(
        "azure_ambiguous",
        "azure-function-other",
        Some("https://example.com/azure/other.git"),
    );
    let path = infra.join("main.tf");
    std::fs::write(
        &path,
        std::fs::read_to_string(&path).unwrap().replace(
            "image_name = \"azure/functions\"",
            "image_name = \"azure/functions-${var.direction}\"",
        ),
    )
    .unwrap();
    std::fs::write(infra.join("meada.toml"), "[[peer]]\nrepo = \"example.com/azure/app\"\n[[peer]]\nrepo = \"example.com/azure/other\"\n[serve]\n").unwrap();
    std::fs::write(app.join(".github/workflows/image.yaml"), "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/azure/functions-north:v1\n").unwrap();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    assert!(
        links.iter().all(|l| l["level"] == "ambiguous"
            && l["contract"]
                .as_str()
                .unwrap()
                .starts_with("image::ghcr.io/azure/functions-")),
        "{text}"
    );
    assert_eq!(json["executions"].as_array().unwrap().len(), 2, "{text}");
    let resolved: Value =
        serde_json::from_str(&Engine::open(&app).unwrap().export().unwrap()).unwrap();
    assert!(resolved.get("provisions").is_none(), "{resolved}");
    assert_eq!(
        resolved["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["contract"] == "channel::sqs")
            .count(),
        4,
        "{resolved}"
    );
}

#[test]
fn composed_subscription_targets_preserve_physical_names_overrides_and_unknown_refusal() {
    let (infra, app) = workspace("azure_composition");
    std::fs::write(infra.join("main.tf"), "variable \"topic\" { default = \"orders\" }\nmodule \"topic\" {\n source = \"./topic\"\n topic = var.topic\n}\nresource \"azurerm_servicebus_subscription\" \"audit\" {\n name = \"audit\"\n topic_id = module.topic.id\n}\nresource \"azurerm_servicebus_subscription\" \"unknown\" {\n name = \"audit\"\n topic_id = data.azurerm_servicebus_topic.existing.id\n}\n").unwrap();
    std::fs::create_dir_all(infra.join("topic")).unwrap();
    std::fs::write(infra.join("topic/main.tf"), "variable \"topic\" {}\nresource \"azurerm_servicebus_topic\" \"this\" { name = var.topic }\noutput \"id\" { value = azurerm_servicebus_topic.this.id }\n").unwrap();
    std::fs::write(
        app.join("settings.yaml"),
        "servicebus_subscription: orders/audit\n",
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 1, "{text}");
    assert_eq!(
        provisions[0]["contract"],
        "channel::servicebus::subscription:orders/audit"
    );
    assert_eq!(provisions[0]["level"], "inferred");
    assert!(
        provisions[0]["reason"]
            .as_str()
            .unwrap()
            .contains("var.topic"),
        "{text}"
    );
    let context = engine
        .context("azurerm_servicebus_subscription.audit", Some(12000))
        .unwrap();
    assert!(
        context.contains("name: \"audit\"") && context.contains("subscription:orders/audit"),
        "{context}"
    );
    let provided: Value =
        serde_json::from_str(&Engine::open(&infra).unwrap().export().unwrap()).unwrap();
    assert_eq!(
        provided["provisioners"].as_array().unwrap().len(),
        2,
        "{provided}"
    );
    std::fs::write(infra.join("terraform.tfvars"), "topic = \"other\"\n").unwrap();
    let changed = engine.export().unwrap();
    let changed: Value = serde_json::from_str(&changed).unwrap();
    assert!(changed.get("provisions").is_none(), "{changed}");
    // A typed key's plain value is `inferred`: no orphan line, and `context` shows it unlinked
    // (#797).
    let (_, reports) = engine.status_report().unwrap();
    let report = reports.into_iter().flatten().collect::<Vec<_>>().join("\n");
    assert!(!report.contains("subscription:orders/audit"), "{report}");
    let context = engine
        .context("channel::servicebus::subscription:orders/audit", None)
        .unwrap();
    assert!(
        context.contains("use settings.yaml#servicebus_subscription (line 1) [inferred]")
            && !context.contains(" provisioner "),
        "{context}"
    );
}

#[test]
fn function_environment_fanout_is_charged_before_copying_images() {
    let infra = fixture_repo(
        "azure_budget",
        "azure-functions",
        Some("https://example.com/azure/infra.git"),
    );
    let app = fixture_repo(
        "azure_budget",
        "azure-function-app",
        Some("https://example.com/azure/app.git"),
    );
    let image = "n".repeat(1200);
    let settings = (0..1000)
        .map(|i| format!(" E{i} = \"tasks\"\n"))
        .collect::<String>();
    let source = format!(
        "resource \"aws_sqs_queue\" \"tasks\" {{ name = \"tasks\" }}\nresource \"azurerm_linux_function_app\" \"image\" {{\n name = \"worker\"\n app_settings = {{\n TASK_QUEUE = \"tasks\"\n{settings} }}\n site_config {{\n application_stack {{\n docker {{\n registry_url = \"https://ghcr.io\"\n image_name = \"{image}\"\n image_tag = \"v1\"\n }}\n }}\n }}\n}}\n"
    );
    std::fs::write(infra.join("main.tf"), &source).unwrap();
    std::fs::write(app.join(".github/workflows/image.yaml"), format!("jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/{image}:v1\n")).unwrap();
    let start = std::time::Instant::now();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let elapsed = start.elapsed();
    eprintln!("Azure app-settings fanout: {elapsed:?}");
    assert!(elapsed < std::time::Duration::from_secs(5));
    let json: Value = serde_json::from_str(&text).unwrap();
    let runner = Engine::open(&infra).unwrap().export().unwrap();
    let runner: Value = serde_json::from_str(&runner).unwrap();
    assert_eq!(
        runner["links"].as_array().map(Vec::len),
        Some(1),
        "{runner}"
    );
    assert!(json.get("provisions").is_none(), "{text}");
    assert_eq!(
        json["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["contract"] == "channel::sqs")
            .count(),
        4,
        "{text}"
    );
    assert!(text.len() < 16000, "{} bytes", text.len());
    std::fs::write(infra.join("main.tf"), source.replace(&settings, "")).unwrap();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 1, "{text}");
    assert_eq!(provisions[0]["contract"], "channel::sqs::tasks");
    assert_eq!(provisions[0]["level"], "inferred");
}

#[test]
fn function_app_settings_can_evaluate_a_checked_in_map() {
    let infra = fixture_repo(
        "azure_setting_map",
        "azure-functions",
        Some("https://example.com/azure/infra.git"),
    );
    let app = fixture_repo(
        "azure_setting_map",
        "azure-function-app",
        Some("https://example.com/azure/app.git"),
    );
    let path = infra.join("main.tf");
    let original = std::fs::read_to_string(&path).unwrap();
    let start = original.find("  app_settings = {").unwrap();
    let end = start + original[start..].find("\n  }\n").unwrap() + "\n  }".len();
    let changed = format!(
        "locals {{\n settings = {{ TASK_QUEUE = \"tasks\", SECRET_QUEUE = \"@Microsoft.KeyVault(SecretUri=https://keys.vault.azure.net/secrets/api-key)\" }}\n}}\n{}  app_settings = local.settings{}",
        &original[..start],
        &original[end..]
    );
    std::fs::write(path, changed).unwrap();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 1, "{text}");
    assert_eq!(provisions[0]["contract"], "channel::sqs::tasks");
    assert_eq!(provisions[0]["level"], "inferred");
    assert_eq!(
        json["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["contract"] == "channel::sqs")
            .count(),
        3,
        "{text}"
    );
}

#[test]
fn queue_arm_names_with_subscription_segments_remain_queue_identities() {
    let (_, app) = workspace("azure_queue_arm");
    std::fs::write(
        app.join("settings.yaml"),
        "queue_id: /subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/ns/queues/orders/subscriptions/audit\nsubscription_id: /subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/ns/topics/events/subscriptions/audit\n",
    )
    .unwrap();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 2, "{text}");
    assert!(provisions.iter().all(|p| p["level"] == "exact"));
    let mut contracts = provisions
        .iter()
        .map(|p| p["contract"].as_str().unwrap())
        .collect::<Vec<_>>();
    contracts.sort();
    assert_eq!(
        contracts,
        [
            "channel::servicebus::orders/subscriptions/audit",
            "channel::servicebus::subscription:events/audit"
        ]
    );
}

#[test]
fn qualified_subscription_patterns_keep_their_source_evidence() {
    let (infra, app) = workspace("azure_subscription_pattern");
    std::fs::write(infra.join("main.tf"), "resource \"azurerm_servicebus_topic\" \"orders\" { name = \"orders\" }\nresource \"azurerm_servicebus_subscription\" \"audit\" {\n name = \"audit-${var.unresolved}\"\n topic_id = azurerm_servicebus_topic.orders.id\n}\n").unwrap();
    std::fs::write(
        app.join("settings.yaml"),
        "servicebus_subscription: orders/audit-prod\n",
    )
    .unwrap();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 1, "{text}");
    assert_eq!(
        provisions[0]["contract"],
        "channel::servicebus::subscription:orders/audit-prod"
    );
    assert_eq!(provisions[0]["level"], "inferred");
    assert!(
        provisions[0]["reason"]
            .as_str()
            .unwrap()
            .contains("; pattern")
    );
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    assert!(
        json["provisioners"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "channel::servicebus::subscription:orders/audit-*"),
        "{text}"
    );
}

#[test]
fn azure_resource_references_reject_attributes_the_provider_does_not_expose() {
    let (infra, app) = workspace("azure_wrong_attribute");
    std::fs::write(infra.join("main.tf"), "resource \"azurerm_servicebus_topic\" \"events\" { name = \"events\" }\nresource \"azurerm_servicebus_subscription\" \"audit\" {\n name = \"audit\"\n topic_id = azurerm_servicebus_topic.events.arn\n}\n").unwrap();
    std::fs::write(
        app.join("settings.yaml"),
        "servicebus_subscription: events/audit\n",
    )
    .unwrap();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["uses"].as_array().map(Vec::len), Some(1), "{text}");
    assert!(json.get("provisions").is_none(), "{text}");
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        json["provisioners"].as_array().map(Vec::len),
        Some(1),
        "{text}"
    );
    assert!(json.get("subscriptions").is_none(), "{text}");
}

#[test]
fn terraform_json_functions_share_typed_environment_and_projection_facts() {
    let infra = fixture_repo(
        "azure_json",
        "azure-functions",
        Some("https://example.com/azure/infra.git"),
    );
    let app = fixture_repo(
        "azure_json",
        "azure-function-app",
        Some("https://example.com/azure/app.git"),
    );
    std::fs::remove_file(infra.join("main.tf")).unwrap();
    let private = "https://sqs.us-east-1.amazonaws.com/123456789012/tasks";
    let source = serde_json::json!({"variable": {"private_queue": {"sensitive": true, "default": private}}, "resource": {
        "aws_sqs_queue": {"tasks": {"name": "tasks"}},
        "azurerm_key_vault_secret": {"token": {"name": "private-queue", "value": "${var.private_queue}"}},
        "azurerm_servicebus_topic": {"events": {"name": "events"}},
        "azurerm_servicebus_subscription": {"audit": {"name": "audit", "topic_id": "${azurerm_servicebus_topic.events.id}"}},
        "azurerm_linux_function_app": {"image": {
            "name": "image-worker", "app_settings": {"TASK_QUEUE": "${aws_sqs_queue.tasks.id}", "SECRET_QUEUE": "${azurerm_key_vault_secret.token.value}"},
            "site_config": [{"application_stack": [{"docker": [{"registry_url": "https://ghcr.io", "image_name": "azure/functions", "image_tag": "${var.unresolved}"}]}]}]
        }},
        "azurerm_windows_function_app": {"archive": {"name": "archive-worker", "zip_deploy_file": "worker.zip"}}
    }});
    std::fs::write(infra.join("main.tf.json"), source.to_string()).unwrap();
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["executions"].as_array().unwrap().len(), 1, "{text}");
    assert!(json.get("uses").is_none(), "{text}");
    let context = Engine::open(&infra)
        .unwrap()
        .search("variable.private_queue.default", Some("value"), None)
        .unwrap();
    assert!(
        context.contains("redacted") && !context.contains(private),
        "{context}"
    );
    let index = std::fs::read(infra.join(".meada/index.db")).unwrap();
    assert!(
        !index
            .windows(private.len())
            .any(|bytes| bytes == private.as_bytes())
    );
    assert!(
        json["provisioners"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["contract"] == "channel::servicebus::subscription:events/audit"),
        "{text}"
    );
    let text = Engine::open(&app).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let provisions = json["provisions"].as_array().unwrap();
    assert_eq!(provisions.len(), 1, "{text}");
    assert_eq!(provisions[0]["contract"], "channel::sqs::tasks");
    assert_eq!(provisions[0]["level"], "inferred");
    assert_eq!(
        json["providers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["contract"] == "channel::sqs")
            .count(),
        3,
        "{text}"
    );
}
