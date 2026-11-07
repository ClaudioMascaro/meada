//! AWS provisioners and subscription routing (#658, ADR 0026).
use crate::common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

fn workspace(case: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let infra = fixture_repo(case, "aws-infra", Some("https://example.com/aws/infra.git"));
    let app = fixture_repo(case, "aws-app", Some("https://example.com/aws/app.git"));
    (infra, app)
}

#[test]
fn aws_kinds_provision_literal_and_arn_names_in_every_root() {
    let (infra, _) = workspace("aws_kinds");
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let expected = [
        "channel::sns::alerts",
        "channel::eventbridge::events",
        "channel::kinesis::records",
        "resource::dynamodb::orders",
        "resource::rds::orders-db",
        "resource::rds::orders-cluster",
        "resource::elasticache::orders-cache",
        "resource::elasticache::orders-group",
        "resource::ssm::/orders/token",
        "resource::secretsmanager::orders/token",
    ];
    let sides = json["provisioners"].as_array().expect("AWS provisioners");
    assert_eq!(sides.len(), 20, "{text}");
    let links = json["provisions"].as_array().unwrap();
    assert_eq!(links.len(), 20, "{text}");
    for contract in expected {
        for root in ["literal", "arn"] {
            assert_eq!(
                sides
                    .iter()
                    .filter(|p| p["contract"] == contract && p["root"] == root)
                    .count(),
                1,
                "{text}"
            );
            let link = links
                .iter()
                .find(|l| l["contract"] == contract && l["root"] == root)
                .unwrap();
            assert_eq!(
                link["level"],
                if contract.starts_with("channel::") {
                    "exact"
                } else {
                    "declared"
                }
            );
        }
    }
    let mut export = meada_core::export::parse(&text).unwrap();
    export.commit = None;
    export.dirty = None;
    export.processes.clear();
    assert_golden("aws-infra.export.json", &export.write().unwrap());
}

#[test]
fn secure_string_values_are_redacted_and_refresh_when_the_type_changes() {
    let (infra, _) = workspace("aws_secure_string");
    let path = infra.join("literal/main.tf");
    let original = std::fs::read_to_string(&path).unwrap();
    let secret = "aws-ticket-secret-value";
    let text = original.replace(
        "resource \"aws_ssm_parameter\" \"parameter\" { name = \"/orders/token\" }",
        &format!("resource \"aws_ssm_parameter\" \"parameter\" {{\n name = \"/orders/token\"\n type = \"SecureString\"\n value = \"{secret}\"\n}}"),
    );
    assert_ne!(text, original);
    std::fs::write(&path, &text).unwrap();
    let engine = Engine::open(&infra).unwrap();
    let search = engine
        .search(
            "resource.aws_ssm_parameter.parameter.value",
            Some("value"),
            Some(8000),
        )
        .unwrap();
    assert!(search.contains("redacted"), "{search}");
    assert_eq!(
        engine.search(secret, Some("value"), None).unwrap(),
        format!("no matches for `{secret}`\n")
    );
    assert!(
        !engine
            .context("aws_ssm_parameter.parameter (root literal)", Some(8000))
            .unwrap()
            .contains(secret)
    );
    assert!(!engine.export().unwrap().contains(secret));
    std::fs::write(&path, text.replace("SecureString", "String")).unwrap();
    assert!(
        engine
            .search(secret, Some("value"), None)
            .unwrap()
            .contains(secret)
    );
    std::fs::write(&path, text).unwrap();
    assert_eq!(
        engine.search(secret, Some("value"), None).unwrap(),
        format!("no matches for `{secret}`\n")
    );
}

fn routing_workspace(case: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let (infra, app) = workspace(case);
    std::fs::create_dir_all(infra.join("routes")).unwrap();
    std::fs::write(
        infra.join("routes/main.tf"),
        include_str!("../fixtures/aws-routes/main.tf"),
    )
    .unwrap();
    std::fs::write(
        app.join("app.py"),
        include_str!("../fixtures/aws-routes/app.py"),
    )
    .unwrap();
    (infra, app)
}

#[test]
fn aws_subscriptions_link_target_consumers_to_source_producers_and_trace_them() {
    use meada::engine::{Direction, Start};
    let (infra, app) = routing_workspace("aws_subscriptions");
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let links = json["links"].as_array().unwrap();
    assert_eq!(links.len(), 4, "{text}");
    for (consumer, broker, level, subscription) in [
        (
            "sns_receive",
            "sns send alerts",
            "exact",
            "aws_sns_topic_subscription.plain",
        ),
        (
            "filtered_receive",
            "sns send alerts",
            "inferred",
            "aws_sns_topic_subscription.filtered",
        ),
        (
            "bus_receive",
            "eventbridge send events",
            "exact",
            "aws_cloudwatch_event_target.plain",
        ),
        (
            "pattern_receive",
            "eventbridge send events",
            "inferred",
            "aws_cloudwatch_event_target.pattern",
        ),
    ] {
        let l = links
            .iter()
            .find(|l| {
                l["consumer"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("::{consumer}"))
            })
            .unwrap();
        assert!(l["provider"].as_str().unwrap().ends_with(broker), "{l}");
        assert_eq!(l["level"], level, "{l}");
        assert!(l["reason"].as_str().unwrap().contains(subscription), "{l}");
        let up = engine
            .trace(Start::From(consumer), Direction::Up, Some(12000))
            .unwrap();
        assert!(up.contains(broker) && up.contains(subscription), "{up}");
    }
    let down = engine
        .trace(
            Start::From("channel::sns::alerts"),
            Direction::Down,
            Some(12000),
        )
        .unwrap();
    assert!(
        down.contains("sns_receive") && down.contains("filtered_receive"),
        "{down}"
    );
    let context = engine
        .context("channel::sqs::sns-jobs", Some(12000))
        .unwrap();
    assert!(
        context.contains("sns send alerts") && context.contains("subscription"),
        "{context}"
    );
    assert_golden("aws-subscriptions.context.txt", &context);
    // The infrastructure view walks references and provisioning links to the same consumers.
    let from_infra = Engine::open(&infra)
        .unwrap()
        .trace(
            Start::From("aws_sns_topic.alerts"),
            Direction::Down,
            Some(12000),
        )
        .unwrap();
    assert!(from_infra.contains("sns_receive"), "{from_infra}");
}

#[test]
fn aws_orphans_are_reported_per_kind_only_when_the_workspace_has_both_sides() {
    let (infra, _) = workspace("aws_orphans");
    let path = infra.join("literal/main.tf");
    let original = std::fs::read_to_string(&path).unwrap();
    let changed = original
        .replace("alerts\"", "unused-alerts\"")
        .replace("events\"", "unused-events\"")
        .replace("records\"", "unused-records\"")
        .replace("orders\"", "unused-orders\"")
        .replace("orders-db\"", "unused-db\"")
        .replace("orders-cluster\"", "unused-cluster\"")
        .replace("orders-cache\"", "unused-cache\"")
        .replace("orders-group\"", "unused-group\"")
        .replace("/orders/token\"", "/unused/token\"")
        .replace("orders/token\"", "unused/token\"");
    std::fs::write(&path, changed).unwrap();
    let engine = Engine::open(&infra).unwrap();
    let (_, reports) = engine.status_report().unwrap();
    let text = reports.into_iter().flatten().collect::<Vec<_>>().join("\n");
    for name in [
        "channel::sns::unused-alerts",
        "channel::eventbridge::unused-events",
        "channel::kinesis::unused-records",
        "resource::dynamodb::unused-orders",
        "resource::rds::unused-db",
        "resource::rds::unused-cluster",
        "resource::elasticache::unused-cache",
        "resource::elasticache::unused-group",
        "resource::ssm::/unused/token",
        "resource::secretsmanager::unused/token",
    ] {
        assert!(
            text.contains(&format!("orphan {name}: no use;")),
            "{name}: {text}"
        );
    }
    assert_eq!(
        text.lines().filter(|l| l.starts_with("orphan ")).count(),
        10,
        "{text}"
    );
    assert_eq!(text.lines().count(), 20, "{text}");
    assert_golden("aws-orphans.status.txt", &text);
    // Removing all SSM provisioners reports the unmatched use only while another SSM
    // provisioner is present; a wholly absent kind must not produce a status warning.
    for dir in ["literal", "arn"] {
        let path = infra.join(dir).join("main.tf");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            path,
            text.lines()
                .filter(|l| !l.contains("aws_ssm_parameter"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
    }
    let (_, reports) = engine.status_report().unwrap();
    let text = reports.into_iter().flatten().collect::<Vec<_>>().join("\n");
    assert!(
        !text
            .lines()
            .any(|l| l.contains("resource::ssm::") && l.contains("no provisioner")),
        "{text}"
    );
}

#[test]
fn subscription_edits_refresh_links_and_partial_queries_equal_the_full_export() {
    use meada::engine::{Direction, Start};
    let (infra, app) = routing_workspace("aws_refresh");
    let engine = Engine::open(&app).unwrap();
    let before: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let queries = || {
        let mut text = String::new();
        for q in [
            "sns_receive",
            "bus_receive",
            "channel::sns::alerts",
            "channel::sqs::sns-jobs",
            "aws_sns_topic.alerts",
        ] {
            text += &engine.context(q, Some(12000)).unwrap();
            text += &engine
                .trace(Start::From(q), Direction::Up, Some(12000))
                .unwrap();
            text += &engine
                .trace(Start::From(q), Direction::Down, Some(12000))
                .unwrap();
        }
        text
    };
    assert_eq!(queries(), meada::engine::with_every_row(queries));
    let path = infra.join("routes/main.tf");
    let original = std::fs::read_to_string(&path).unwrap();
    // Same-size endpoint change, followed by restore, exercises the warm summary refresh.
    std::fs::write(
        &path,
        original.replace("aws_sqs_queue.sns.arn", "aws_sqs_queue.bus.arn"),
    )
    .unwrap();
    let edited: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let links = edited["links"].as_array().unwrap();
    assert_eq!(links.len(), 4, "{edited}");
    assert!(
        !links
            .iter()
            .any(|l| l["consumer"].as_str().unwrap().ends_with("::sns_receive")),
        "{edited}"
    );
    assert_eq!(
        links
            .iter()
            .filter(|l| l["consumer"].as_str().unwrap().ends_with("::bus_receive"))
            .count(),
        2
    );
    drop(engine);
    std::fs::remove_dir_all(infra.join(".meada")).unwrap();
    std::fs::remove_dir_all(app.join(".meada")).unwrap();
    let engine = Engine::open(&app).unwrap();
    let cold: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(edited, cold);
    std::fs::write(path, original).unwrap();
    let restored: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(restored, before);
}

#[test]
fn secure_string_type_expressions_do_not_expose_a_value() {
    let (infra, _) = workspace("aws_secret_type_expression");
    let path = infra.join("secret.tf");
    std::fs::write(path, "variable \"kind\" { default = \"SecureString\" }\nresource \"aws_ssm_parameter\" \"typed\" {\n name = \"/typed/secret\"\n type = var.kind\n value = \"typed-secret-bytes\"\n}\n").unwrap();
    let engine = Engine::open(&infra).unwrap();
    let text = engine
        .search(
            "resource.aws_ssm_parameter.typed.value",
            Some("value"),
            None,
        )
        .unwrap();
    assert!(text.contains("redacted"), "{text}");
}

#[test]
fn aws_subscription_summaries_export_their_source_target_and_declaration() {
    let (infra, _) = routing_workspace("aws_subscription_export");
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let summaries = data["subscriptions"]
        .as_array()
        .expect("ADR 0026 subscription summaries");
    assert_eq!(summaries.len(), 4, "{text}");
    for summary in summaries {
        assert!(
            summary["source"].as_str().unwrap().starts_with("channel::"),
            "{summary}"
        );
        assert!(
            summary["target"]
                .as_str()
                .unwrap()
                .starts_with("channel::sqs::"),
            "{summary}"
        );
        assert!(
            summary["node"]
                .as_str()
                .unwrap()
                .starts_with("example.com/aws/infra::routes/main.tf::aws_"),
            "{summary}"
        );
        assert_eq!(summary["root"], "routes");
    }
    let mut export = meada_core::export::parse(&text).unwrap();
    export.commit = None;
    export.dirty = None;
    export.processes.clear();
    assert_golden("aws-subscriptions.export.json", &export.write().unwrap());
}

#[test]
fn aws_wrong_service_arns_remain_nodes_without_provisioning_a_contract() {
    let (infra, _) = workspace("aws_wrong_service");
    std::fs::write(infra.join("bad.tf"), "resource \"aws_sns_topic\" \"wrong\" { name = \"arn:aws:sqs:us-east-1:123456789012:alerts\" }\nresource \"aws_dynamodb_table\" \"wrong\" { name = \"arn:aws:s3:::orders\" }\n").unwrap();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(data["provisioners"].as_array().unwrap().len(), 20, "{text}");
    assert!(
        engine
            .search("aws_sns_topic.wrong", Some("iac"), None)
            .unwrap()
            .contains("bad.tf")
    );
}

#[test]
fn subscription_endpoints_follow_module_arguments_outputs_and_instance_keys() {
    let (infra, app) = workspace("aws_module_routes");
    std::fs::create_dir_all(infra.join("module")).unwrap();
    std::fs::write(
        infra.join("module/main.tf"),
        r#"
variable "topic" {}
variable "queue" {}
resource "aws_sns_topic_subscription" "from_module" {
 topic_arn = var.topic
 endpoint = var.queue
 protocol = "sqs"
}
output "queue" { value = var.queue }
"#,
    )
    .unwrap();
    std::fs::write(
        infra.join("main.tf"),
        r#"
resource "aws_sns_topic" "topics" {
 for_each = toset(["alerts"])
 name = each.key
}
resource "aws_sqs_queue" "queues" {
 for_each = toset(["jobs"])
 name = each.key
}
module "routing" {
 source = "./module"
 topic = aws_sns_topic.topics["alerts"].arn
 queue = aws_sqs_queue.queues["jobs"].arn
}
resource "aws_sns_topic_subscription" "instance" {
 for_each = toset(["jobs"])
 topic_arn = aws_sns_topic.topics["alerts"].arn
 endpoint = aws_sqs_queue.queues[each.key].arn
 protocol = "sqs"
}
resource "aws_sns_topic_subscription" "output" {
 topic_arn = aws_sns_topic.topics["alerts"].arn
 endpoint = module.routing.queue
 protocol = "sqs"
}
"#,
    )
    .unwrap();
    std::fs::write(app.join("app.py"), "import boto3\nqueue = boto3.client(\"sqs\")\ndef receive():\n return queue.receive_message(QueueUrl=\"https://sqs.us-east-1.amazonaws.com/123456789012/jobs\")\n").unwrap();
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let summaries = data["subscriptions"].as_array().unwrap();
    assert_eq!(summaries.len(), 3, "{text}");
    assert!(
        summaries
            .iter()
            .all(|s| s["source"] == "channel::sns::alerts" && s["target"] == "channel::sqs::jobs")
    );
}

#[test]
fn overridable_instance_selection_is_inferred_for_resources_and_module_outputs() {
    for shape in ["resource", "module"] {
        let (infra, app) = workspace(&format!("aws_selection_{shape}"));
        std::fs::create_dir_all(infra.join("queue_module")).unwrap();
        std::fs::write(infra.join("queue_module/main.tf"), "variable \"name\" {}\nresource \"aws_sqs_queue\" \"this\" { name = var.name }\noutput \"arn\" { value = aws_sqs_queue.this.arn }\n").unwrap();
        let (queues, endpoint) = if shape == "resource" {
            (
                "resource \"aws_sqs_queue\" \"queues\" {\n for_each = toset([\"jobs\", \"more\"])\n name = each.key\n}\n",
                "aws_sqs_queue.queues[var.pick].arn",
            )
        } else {
            (
                "module \"queues\" {\n for_each = toset([\"jobs\", \"more\"])\n source = \"./queue_module\"\n name = each.key\n}\n",
                "module.queues[var.pick].arn",
            )
        };
        std::fs::write(infra.join("main.tf"), format!("variable \"pick\" {{ default = \"jobs\" }}\nresource \"aws_sns_topic\" \"source\" {{ name = \"alerts\" }}\n{queues}resource \"aws_sns_topic_subscription\" \"selected\" {{\n topic_arn = aws_sns_topic.source.arn\n protocol = \"sqs\"\n endpoint = {endpoint}\n}}\n")).unwrap();
        std::fs::write(app.join("app.py"), "import boto3\nq = boto3.client(\"sqs\")\ndef receive_jobs():\n return q.receive_message(QueueUrl=\"https://sqs.us-east-1.amazonaws.com/123456789012/jobs\")\ndef receive_more():\n return q.receive_message(QueueUrl=\"https://sqs.us-east-1.amazonaws.com/123456789012/more\")\n").unwrap();
        let engine = Engine::open(&app).unwrap();
        for selected in ["jobs", "more"] {
            let path = infra.join("main.tf");
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                path,
                text.replace("default = \"jobs\"", &format!("default = \"{selected}\"")),
            )
            .unwrap();
            let text = engine.export().unwrap();
            let data: Value = serde_json::from_str(&text).unwrap();
            let links = data["links"].as_array().unwrap();
            assert_eq!(links.len(), 1, "{shape}: {text}");
            assert!(
                links[0]["consumer"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("::receive_{selected}")),
                "{shape}: {text}"
            );
            assert_eq!(links[0]["level"], "inferred", "{shape}: {text}");
            assert!(
                links[0]["reason"]
                    .as_str()
                    .unwrap()
                    .contains("var.pick (default)"),
                "{shape}: {text}"
            );
        }
    }
}

#[test]
fn subscription_endpoint_alias_cycles_and_deep_chains_stay_bounded() {
    let (infra, app) = workspace("aws_alias_bounds");
    let mut aliases = String::from(
        "locals {\n cycle_a = local.cycle_b\n cycle_b = local.cycle_a\n short0 = aws_sns_topic.source.arn\n long0 = aws_sns_topic.source.arn\n",
    );
    for n in 1..80 {
        aliases += &format!(" long{n} = local.long{}\n", n - 1);
        if n < 8 {
            aliases += &format!(" short{n} = local.short{}\n", n - 1);
        }
    }
    aliases += "}\nresource \"aws_sns_topic\" \"source\" { name = \"alerts\" }\n";
    for (key, expr) in [
        ("short", "local.short7"),
        ("deep", "local.long79"),
        ("cycle", "local.cycle_a"),
    ] {
        aliases += &format!(
            "resource \"aws_sns_topic_subscription\" \"{key}\" {{\n topic_arn = {expr}\n protocol = \"sqs\"\n endpoint = \"arn:aws:sqs:us-east-1:123456789012:jobs\"\n}}\n"
        );
    }
    std::fs::write(infra.join("main.tf"), aliases).unwrap();
    let started = std::time::Instant::now();
    let text = Engine::open(&infra).unwrap().export().unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
    let data: Value = serde_json::from_str(&text).unwrap();
    let subscriptions = data["subscriptions"].as_array().unwrap();
    assert_eq!(subscriptions.len(), 1, "{text}");
    assert!(
        subscriptions[0]["node"]
            .as_str()
            .unwrap()
            .ends_with("::aws_sns_topic_subscription.short")
    );
    assert!(app.is_dir());
    eprintln!("subscription alias chain/cycle probe: {elapsed:?}");
}

#[test]
fn eventbridge_timer_only_rules_do_not_route_events_published_to_the_bus() {
    let (infra, app) = routing_workspace("aws_timer_rule");
    let path = infra.join("routes/main.tf");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text += "\nresource \"aws_cloudwatch_event_rule\" \"timer_only\" {\n name = \"timer\"\n event_bus_name = \"events\"\n schedule_expression = \"rate(5 minutes)\"\n}\nresource \"aws_cloudwatch_event_target\" \"timer_only\" {\n rule = \"timer\"\n event_bus_name = \"events\"\n arn = \"arn:aws:sqs:us-east-1:123456789012:timer-jobs\"\n}\n";
    std::fs::write(path, text).unwrap();
    let mut code = std::fs::read_to_string(app.join("app.py")).unwrap();
    code += "\ndef timer_receive():\n return queue.receive_message(QueueUrl=\"https://sqs.us-east-1.amazonaws.com/123456789012/timer-jobs\")\n";
    std::fs::write(app.join("app.py"), code).unwrap();
    let text = Engine::open(&app).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let links = data["links"].as_array().unwrap();
    assert_eq!(links.len(), 4, "{text}");
    assert!(
        !links
            .iter()
            .any(|l| l["consumer"].as_str().unwrap().ends_with("::timer_receive")),
        "{text}"
    );
}

#[test]
fn secure_string_values_are_redacted_in_terraform_and_opentofu_json() {
    for suffix in ["tf.json", "tofu.json"] {
        let (infra, _) = workspace(&format!("aws_secure_{}", suffix.replace('.', "_")));
        std::fs::write(infra.join(format!("secret.{suffix}")), r#"{"resource":{"aws_ssm_parameter":{"secret":{"name":"/secret","type":"SecureString","value":"json-secret-sentinel"},"ordinary":{"name":"/ordinary","type":"String","value":"ordinary-parameter"},"dynamic":{"name":"/dynamic","type":"${var.kind}","value":"dynamic-secret-sentinel"}}}}"#).unwrap();
        let engine = Engine::open(&infra).unwrap();
        for name in ["secret", "dynamic"] {
            let text = engine
                .search(
                    &format!("resource.aws_ssm_parameter.{name}.value"),
                    Some("value"),
                    None,
                )
                .unwrap();
            assert!(text.contains("redacted"), "{suffix}: {text}");
        }
        for secret in ["json-secret-sentinel", "dynamic-secret-sentinel"] {
            assert_eq!(
                engine.search(secret, Some("value"), None).unwrap(),
                format!("no matches for `{secret}`\n")
            );
        }
        assert!(
            engine
                .search("ordinary-parameter", Some("value"), None)
                .unwrap()
                .contains("ordinary-parameter")
        );
    }
}
