//! IaC nodes (#649, ADR 0025): every Terraform resource, data block and module call with its
//! address and root module, found by `search` with the kind `iac`, explained by `context` and
//! walked by `trace` through the references between them.

use std::fs;

use meada::engine::{Direction, Engine, Start};

use crate::common::{assert_golden, fixture_repo};

const ORIGIN: Option<&str> = Some("https://example.com/acme/infra.git");

/// Each answer of `ask` over `questions`, in order, a blank line between two.
fn answers(questions: &[&str], ask: impl Fn(&str) -> String) -> String {
    (questions.iter())
        .map(|q| ask(q))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fixture: a root module `envs/prod` with resources, a data block, a `count`, a `for_each`
/// and a registry module, calling the local module `modules/queue` twice and `modules/worker`
/// once with an output of the first; and a root `envs/stage` calling `modules/queue` once. A
/// query matches the address, the type or the physical name, exact first, then prefix, then
/// substring.
#[test]
fn search_with_the_kind_iac_finds_nodes_by_address_type_and_physical_name() {
    let repo = fixture_repo("iac_search", "terraform-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let queries = [
        "aws_sqs_queue",
        "dead-letters",
        "module.orders",
        "data.aws_caller_identity.current",
        "module",
        "nothing-here",
    ];
    let search = |q: &str| engine.search(q, Some("iac"), None).unwrap();
    assert_golden("terraform-nodes.search.txt", &answers(&queries, search));
}

/// `context` on a node shows its type, physical name, root, its references both ways (each
/// through the name it went by) and its config values; an address two roots hold lists both,
/// each with what to pass.
#[test]
fn context_on_a_node_shows_its_type_name_root_references_and_values() {
    let repo = fixture_repo("iac_context", "terraform-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let questions = [
        "aws_sns_topic.orders",
        "module.orders.aws_sqs_queue.this",
        "module.orders.aws_sqs_queue.this (root envs/prod)",
        "envs/prod/main.tf::aws_sqs_queue.dlq[1]",
        "example.com/acme/infra::envs/prod/main.tf::module.audit",
        "module.worker.aws_lambda_event_source_mapping.this",
    ];
    let context = |q: &str| engine.context(q, None).unwrap();
    assert_golden("terraform-nodes.context.txt", &answers(&questions, context));
}

/// `trace` walks the references down (what a node reaches, into the local modules through their
/// inputs and outputs) and up (what reaches it).
#[test]
fn trace_walks_the_references_of_a_node_both_ways() {
    let repo = fixture_repo("iac_trace", "terraform-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let down = |q: &str| (engine.trace(Start::From(q), Direction::Down, None)).unwrap();
    let up = |q: &str| engine.trace(Start::From(q), Direction::Up, None).unwrap();
    let text = [
        answers(&["module.orders (root envs/prod)"], down),
        answers(
            &["module.worker.aws_lambda_event_source_mapping.this"],
            down,
        ),
        answers(&["aws_cloudwatch_metric_alarm.depth[\"orders\"]"], down),
        answers(&["aws_sns_topic.orders"], up),
        answers(&["module.orders.aws_sqs_queue.this (root envs/prod)"], up),
        answers(&["module.orders.aws_sqs_queue.this"], up),
    ];
    assert_golden("terraform-nodes.trace.txt", &text.join("\n"));
}

/// The default search leaves IaC nodes out (ADR 0025): a node's address or physical name finds
/// only the config values that hold it, as it did before nodes were read.
#[test]
fn the_default_search_leaves_iac_nodes_out() {
    let repo = fixture_repo("iac_default", "terraform-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let address = engine.search("aws_sns_topic.orders", None, None).unwrap();
    assert_eq!(
        address,
        engine
            .search("aws_sns_topic.orders", Some("value"), None)
            .unwrap()
    );
    assert!(!address.contains("root envs/prod"), "{address}");
    let name = engine.search("dead-letters", None, None).unwrap();
    assert_eq!(
        name,
        engine.search("dead-letters", Some("value"), None).unwrap()
    );
    assert!(name.contains("resource.aws_sqs_queue.dlq.name"), "{name}");
    let iac = engine.search("dead-letters", Some("iac"), None).unwrap();
    assert!(iac.contains("aws_sqs_queue.dlq[0]"), "{iac}");
}

/// A symbol of the code with the name of a node keeps its `context` and `trace` answers: a node
/// answers only when nothing of the code has the name, and its path-qualified address reaches it.
#[test]
fn a_symbol_named_like_a_node_keeps_its_answers() {
    let repo = fixture_repo("iac_symbol", "terraform-nodes", ORIGIN);
    fs::write(
        repo.join("envs/prod/code.py"),
        "class aws_sns_topic:\n    def orders(self):\n        return 1\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let context = engine.context("aws_sns_topic.orders", None).unwrap();
    assert!(
        context.contains("envs/prod/code.py:2-3 aws_sns_topic.orders"),
        "{context}"
    );
    assert!(!context.contains("type: aws_sns_topic"), "{context}");
    let trace = engine
        .trace(Start::From("aws_sns_topic.orders"), Direction::Up, None)
        .unwrap();
    assert!(
        trace.starts_with("example.com/acme/infra envs/prod/code.py:2"),
        "{trace}"
    );
    let node = engine
        .context("envs/prod/main.tf::aws_sns_topic.orders", None)
        .unwrap();
    assert!(node.contains("type: aws_sns_topic"), "{node}");
}

/// A budget too small for the walk cuts the node's trace, never the answer that no code has
/// the name (prove round 2 on 750403d: budgets up to 18 hid the node).
#[test]
fn a_small_budget_cuts_the_trace_of_a_node() {
    let repo = fixture_repo("iac_budget", "terraform-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let from = Start::From("aws_sns_topic.orders");
    let whole = engine.trace(from, Direction::Up, None).unwrap();
    let rows = whole.lines().count() - 1;
    for budget in [1, 5, 18] {
        let cut = engine.trace(from, Direction::Up, Some(budget)).unwrap();
        assert!(
            cut.contains(&format!(" of {rows} lines (budget {budget} tokens)")),
            "{cut}"
        );
    }
}

/// Nodes with no contract side and references stay out of `meada export` (ADR 0026).
#[test]
fn only_iac_nodes_holding_contract_sides_enter_the_export() {
    let repo = fixture_repo("iac_export", "terraform-nodes", ORIGIN);
    let text = Engine::open(&repo).unwrap().export().unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let nodes = json["iac_nodes"].as_array().unwrap();
    assert!(!nodes.is_empty());
    assert!(nodes.iter().all(|n| n["type"] == "aws_sqs_queue"));
    assert!(json.get("iac_references").is_none());
}

/// An edited, an added and a deleted Terraform file change the nodes and references at the next
/// query, as a cold index of the same files has them.
#[test]
fn a_changed_terraform_file_changes_the_nodes_at_the_next_query() {
    let repo = fixture_repo("iac_fresh", "terraform-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let ask = |engine: &Engine| {
        let mut text = engine.search("aws", Some("iac"), None).unwrap();
        text += &engine
            .trace(Start::From("aws_sns_topic.orders"), Direction::Up, None)
            .unwrap();
        text
    };
    let before = ask(&engine);
    let main = repo.join("envs/prod/main.tf");
    let edited = fs::read_to_string(&main)
        .unwrap()
        .replace("module \"payments\"", "module \"billing\"")
        .replace("module.payments.queue_name", "module.billing.queue_name");
    fs::write(&main, edited).unwrap();
    fs::write(
        repo.join("envs/stage/topic.tf"),
        "resource \"aws_sns_topic\" \"orders\" {}\n",
    )
    .unwrap();
    fs::remove_file(repo.join("modules/worker/main.tf")).unwrap();
    let after = ask(&engine);
    assert_ne!(before, after);
    assert!(
        after.contains("module.billing.aws_sqs_queue.this"),
        "{after}"
    );
    assert!(!after.contains("module.payments"), "{after}");
    assert!(!after.contains("module.worker.aws_lambda"), "{after}");
    assert!(
        after.contains("envs/stage/topic.tf:1 aws_sns_topic.orders"),
        "{after}"
    );
    // A deletion alone, with no other Terraform file read again.
    fs::remove_file(repo.join("envs/stage/topic.tf")).unwrap();
    let deleted = ask(&engine);
    assert!(!deleted.contains("envs/stage/topic.tf"), "{deleted}");
    fs::remove_dir_all(repo.join(".meada")).unwrap();
    assert_eq!(deleted, ask(&Engine::open(&repo).unwrap()));
}

/// A refresh with no Terraform file changed keeps the stored nodes as they are, rather than
/// evaluating them again: a row rewritten by hand survives it, and an edit to a Terraform file
/// replaces it.
#[test]
fn a_refresh_with_no_terraform_file_changed_keeps_the_nodes() {
    let repo = fixture_repo("iac_kept", "terraform-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    engine.refresh().unwrap();
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    db.execute(
        "UPDATE iac_nodes SET type = 'kept' WHERE address = 'aws_sns_topic.orders'",
        [],
    )
    .unwrap();
    fs::write(repo.join("README.md"), "# infra\n").unwrap();
    let kept = engine
        .search("aws_sns_topic.orders", Some("iac"), None)
        .unwrap();
    assert!(kept.contains("(kept, "), "{kept}");
    let main = repo.join("envs/prod/main.tf");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, format!("{text}\n")).unwrap();
    let fresh = engine
        .search("aws_sns_topic.orders", Some("iac"), None)
        .unwrap();
    assert!(fresh.contains("(aws_sns_topic, "), "{fresh}");
}

/// Compose (#652): a project is a Compose file with its override, each service a node
/// `services.<name>` with `depends_on`, `links` and shared networks as references, its image and
/// build context recorded; `${VAR:-default}` is marked overridable, `${VAR}` stays unresolved, and
/// a build context outside the repository, a URL or an unresolved path is not recorded.
#[test]
fn compose_services_are_nodes_with_references_image_and_build_context() {
    let repo = fixture_repo("iac_compose", "compose-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let search = answers(
        &["service", "services.web", "services.cache", "nothing-here"],
        |q| engine.search(q, Some("iac"), None).unwrap(),
    );
    assert_golden("compose-nodes.search.txt", &search);
    let questions = [
        "services.web",
        "services.api",
        "services.cache",
        "services.escape",
        "services.remote",
        "services.unset",
    ];
    let context = answers(&questions, |q| engine.context(q, None).unwrap());
    assert_golden("compose-nodes.context.txt", &context);
    let down = |q: &str| engine.trace(Start::From(q), Direction::Down, None).unwrap();
    let up = |q: &str| engine.trace(Start::From(q), Direction::Up, None).unwrap();
    let trace = [
        answers(&["services.web", "services.db"], down),
        answers(&["services.db"], up),
    ];
    assert_golden("compose-nodes.trace.txt", &trace.join("\n"));
}

/// A change to the override file changes the nodes at the next query, as a cold index gives them.
#[test]
fn an_edited_override_file_changes_the_compose_nodes() {
    let repo = fixture_repo("iac_compose_edit", "compose-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let before = engine.context("services.cache", None).unwrap();
    assert!(before.contains("redis:7"), "{before}");
    let over = repo.join("stack/compose.override.yaml");
    let text = fs::read_to_string(&over).unwrap();
    fs::write(&over, text.replace("redis:7", "valkey:8")).unwrap();
    let warm = engine.context("services.cache", None).unwrap();
    assert!(warm.contains("valkey:8"), "{warm}");
    let cold = Engine::open(&repo)
        .unwrap()
        .context("services.cache", None)
        .unwrap();
    assert_eq!(warm, cold);
    fs::remove_file(&over).unwrap();
    let gone = engine.search("services.cache", Some("iac"), None).unwrap();
    assert!(!gone.contains("services.cache ("), "{gone}");
}

/// The fixture `terraform-names`: a root `envs/prod` whose names come from locals, variables
/// (defaults, `terraform.tfvars`, `regions.auto.tfvars`), functions, a `for_each` over a literal
/// set and one over a variable, a local module's input and output, `terraform.workspace`, a data
/// source and a registry module; and a root `envs/stage` calling the same module with its own
/// var file.
const NAMES: &str = "terraform-names";

/// Static evaluation (#650): `search` shows each name as it will be created, and a name with an
/// unresolved part as a pattern.
#[test]
fn search_shows_names_as_static_evaluation_resolves_them() {
    let repo = fixture_repo("iac_names_search", NAMES, ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let queries = ["orders", "billing", "alerts", "-dlq", "replica"];
    let search = |q: &str| engine.search(q, Some("iac"), None).unwrap();
    assert_golden("terraform-names.search.txt", &answers(&queries, search));
}

/// `context` on a node shows its evaluated name, `(pattern)` when a part is unresolved, and the
/// overridable values (a variable's default, a var file's value) the name went through.
#[test]
fn context_shows_an_evaluated_name_and_the_overridable_values_it_went_through() {
    let repo = fixture_repo("iac_names_context", NAMES, ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let questions = [
        "aws_sqs_queue.orders",
        "aws_sns_topic.events",
        "aws_sqs_queue.regional[\"us\"]",
        "aws_sqs_queue.replica[\"us\"]",
        "aws_sqs_queue.workspace",
        "aws_sns_topic.account",
        "module.billing.aws_sqs_queue.this (root envs/stage)",
        "aws_sqs_queue.billing_dlq",
        "aws_sqs_queue.audit_dlq",
    ];
    let context = |q: &str| engine.context(q, None).unwrap();
    assert_golden("terraform-names.context.txt", &answers(&questions, context));
}

/// The evaluation unit is a root module with its local module calls (ADR 0025): a changed var
/// file evaluates its root again and keeps every other root's rows as they are (a row rewritten
/// by hand in `envs/prod` survives an edit to `envs/stage/terraform.tfvars`), and the answers then
/// equal a cold index of the same files.
#[test]
fn a_changed_var_file_evaluates_its_unit_again_and_nothing_else() {
    let repo = fixture_repo("iac_names_unit", NAMES, ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    engine.refresh().unwrap();
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    db.execute(
        "UPDATE iac_nodes SET type = 'kept' WHERE address = 'aws_sqs_queue.orders'",
        [],
    )
    .unwrap();
    let ask = |engine: &Engine| engine.search("billing", Some("iac"), None).unwrap();
    assert!(ask(&engine).contains("\"shop-stage-billing-queue\""));
    fs::write(
        repo.join("envs/stage/terraform.tfvars"),
        "environment = \"qa\"\n",
    )
    .unwrap();
    let stage = ask(&engine);
    assert!(stage.contains("\"shop-qa-billing-queue\""), "{stage}");
    assert!(!stage.contains("shop-stage-"), "{stage}");
    let kept = engine
        .search("aws_sqs_queue.orders", Some("iac"), None)
        .unwrap();
    assert!(kept.contains("(kept, "), "{kept}");
    // The var file of `envs/prod` evaluates `envs/prod` again.
    fs::write(
        repo.join("envs/prod/terraform.tfvars"),
        "environment = \"live\"\n",
    )
    .unwrap();
    let prod = engine
        .search("aws_sqs_queue.orders", Some("iac"), None)
        .unwrap();
    assert!(
        prod.contains("(aws_sqs_queue, name \"shop-live-orders\""),
        "{prod}"
    );
    // A var file that leaves its root takes the root back to its defaults.
    fs::remove_file(repo.join("envs/stage/terraform.tfvars")).unwrap();
    let warm = ask(&engine);
    assert!(warm.contains("\"shop-dev-billing-queue\""), "{warm}");
    fs::remove_dir_all(repo.join(".meada")).unwrap();
    assert_eq!(warm, ask(&Engine::open(&repo).unwrap()));
}

/// A Kubernetes manifest added to a repository that holds Compose nodes evaluates the Kubernetes
/// objects again without touching the Compose nodes, and the other way round (they share tables).
#[test]
fn compose_and_kubernetes_nodes_keep_each_other_when_one_changes() {
    let repo = fixture_repo("iac_compose_kube", "compose-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let web = engine.context("services.web", None).unwrap();
    fs::create_dir_all(repo.join("k8s")).unwrap();
    let pod = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: probe\nspec:\n  containers: []\n";
    fs::write(repo.join("k8s/pod.yaml"), pod).unwrap();
    assert!(
        engine
            .search("probe", Some("iac"), None)
            .unwrap()
            .contains("Pod")
    );
    assert_eq!(engine.context("services.web", None).unwrap(), web);
    let over = repo.join("stack/compose.override.yaml");
    fs::write(
        &over,
        fs::read_to_string(&over)
            .unwrap()
            .replace("redis:7", "redis:8"),
    )
    .unwrap();
    assert!(
        engine
            .context("services.cache", None)
            .unwrap()
            .contains("redis:8")
    );
    assert!(
        engine
            .search("probe", Some("iac"), None)
            .unwrap()
            .contains("Pod")
    );
}

/// A template's resource type owns its rows even when its filename is a Compose name. Cold
/// indexing and either reader's warm refresh preserve the other reader's nodes and references.
#[test]
fn a_cloudformation_template_named_compose_keeps_its_nodes_and_references() {
    let repo = fixture_repo("iac_cfn_compose_name", "compose-nodes", ORIGIN);
    fs::create_dir(repo.join("cfn")).unwrap();
    let template = repo.join("cfn/compose.yaml");
    let text = "AWSTemplateFormatVersion: '2010-09-09'\nResources:\n  Queue:\n    Type: AWS::SQS::Queue\n    Properties:\n      QueueName: dispatch-before\n  Consumer:\n    Type: AWS::SNS::Topic\n    DependsOn: Queue\n";
    fs::write(&template, text).unwrap();
    let terraform = repo.join("cfn/main.tf");
    fs::write(&terraform, "resource \"aws_sqs_queue\" \"control\" {}\n").unwrap();
    let engine = Engine::open(&repo).unwrap();
    let consumer = || engine.context("cfn/compose.yaml::Consumer", None).unwrap();
    let before = consumer();
    assert!(before.contains("type: AWS::SNS::Topic"), "{before}");
    assert!(before.contains("Queue via DependsOn"), "{before}");
    let compose = engine.context("services.web", None).unwrap();
    assert!(compose.contains("type: service"), "{compose}");

    fs::write(&template, text.replace("dispatch-before", "dispatch-after")).unwrap();
    let queue = engine.context("cfn/compose.yaml::Queue", None).unwrap();
    assert!(queue.contains("name: \"dispatch-after\""), "{queue}");
    assert!(queue.contains("Consumer via DependsOn"), "{queue}");
    assert_eq!(consumer(), before);
    assert_eq!(engine.context("services.web", None).unwrap(), compose);

    fs::write(&terraform, "resource \"aws_sqs_queue\" \"changed\" {}\n").unwrap();
    assert!(
        engine
            .context("aws_sqs_queue.changed", None)
            .unwrap()
            .contains("aws_sqs_queue")
    );
    assert_eq!(consumer(), before);
    assert_eq!(
        engine.context("cfn/compose.yaml::Queue", None).unwrap(),
        queue
    );

    let over = repo.join("stack/compose.override.yaml");
    fs::write(
        &over,
        fs::read_to_string(&over)
            .unwrap()
            .replace("redis:7", "redis:8"),
    )
    .unwrap();
    assert!(
        engine
            .context("services.cache", None)
            .unwrap()
            .contains("redis:8")
    );
    assert_eq!(consumer(), before);
    assert_eq!(
        engine.context("cfn/compose.yaml::Queue", None).unwrap(),
        queue
    );
}

/// CloudFormation and SAM (#653): the fixture `cloudformation-nodes` holds a SAM application
/// (`sam/template.yaml`), a plain template with parameters (`cfn/stack.yaml`), a JSON template
/// and a file with `Resources` but no version or transform, which is no template. Each resource
/// is a node at its logical id, its type `AWS::...`, its root the template's path, its name
/// resolved within the template.
#[test]
fn cloudformation_and_sam_resources_are_nodes_with_references_and_names() {
    let repo = fixture_repo("iac_cfn", "cloudformation-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let search = answers(
        &[
            "AWS::SQS::Queue",
            "dev",
            "assets-prod",
            "OrdersFunction",
            "Serverless",
            "ignored",
            "nothing-here",
        ],
        |q| engine.search(q, Some("iac"), None).unwrap(),
    );
    assert_golden("cloudformation-nodes.search.txt", &search);
    let questions = [
        "OrdersFunction",
        "OrdersQueue",
        "OrdersTable",
        "JobsQueue",
        "AlertsTopic",
        "AssetsBucket",
        "Database",
        "QueueLogs",
        "SharedName",
        "BucketAlias",
        "Literal",
        "Consumer",
        "JobsQueue (root cfn/stack.yaml)",
        "cfn/stack.json::Stream",
    ];
    let context = answers(&questions, |q| engine.context(q, None).unwrap());
    assert_golden("cloudformation-nodes.context.txt", &context);
    let down = |q: &str| engine.trace(Start::From(q), Direction::Down, None).unwrap();
    let up = |q: &str| engine.trace(Start::From(q), Direction::Up, None).unwrap();
    let trace = [
        answers(&["OrdersFunction", "AlertsTopic", "Consumer"], down),
        answers(&["OrdersQueue", "JobsDeadLetter", "Stream"], up),
    ];
    assert_golden("cloudformation-nodes.trace.txt", &trace.join("\n"));
    // A file with `Resources` and no version or transform is no template.
    let none = engine.search("ignored", Some("iac"), None).unwrap();
    assert!(!none.contains("not-a-template"), "{none}");
}

/// A `NoEcho` parameter's default is stored as `redacted` among the config values, and no name
/// is resolved through it.
#[test]
fn a_no_echo_parameter_keeps_its_default_redacted() {
    let repo = fixture_repo("iac_cfn_noecho", "cloudformation-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let values = engine
        .search("Parameters.DbPassword", Some("value"), None)
        .unwrap();
    assert!(
        values.contains("Parameters.DbPassword.Default = redacted"),
        "{values}"
    );
    assert!(!values.contains("hunter2"), "{values}");
    assert!(
        !engine
            .search("hunter2", Some("value"), None)
            .unwrap()
            .contains("Default")
    );
}

/// A change to a template changes its nodes at the next query, as a cold index gives them; an
/// added template, a deleted one, and a file that stops being a template do the same, and a
/// template no change reached keeps its stored rows (one rewritten by hand survives).
#[test]
fn a_changed_template_is_evaluated_again_alone() {
    let repo = fixture_repo("iac_cfn_edit", "cloudformation-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let ask = |e: &Engine| {
        let names = ["OrdersQueue", "JobsQueue", "Stream", "Extra"];
        answers(&names, |q| e.context(q, None).unwrap())
    };
    let before = ask(&engine);
    assert!(before.contains("name: \"shop-orders-dev\""), "{before}");
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    db.execute(
        "UPDATE iac_nodes SET type = 'AWS::Kept::Kept' WHERE address = 'Stream'",
        [],
    )
    .unwrap();
    // The SAM template changes: its nodes follow, the JSON template's row is not rewritten.
    let sam = repo.join("sam/template.yaml");
    let text = fs::read_to_string(&sam).unwrap();
    fs::write(&sam, text.replace("Default: dev", "Default: qa")).unwrap();
    let edited = ask(&engine);
    assert!(edited.contains("name: \"shop-orders-qa\""), "{edited}");
    assert!(edited.contains("type: AWS::Kept::Kept"), "{edited}");
    db.execute(
        "UPDATE iac_nodes SET type = 'AWS::Kinesis::Stream' WHERE address = 'Stream'",
        [],
    )
    .unwrap();
    // An added template.
    let extra = "AWSTemplateFormatVersion: '2010-09-09'\nResources:\n  Extra:\n    Type: AWS::SNS::Topic\n    Properties:\n      TopicName: extra\n";
    fs::write(repo.join("cfn/extra.yaml"), extra).unwrap();
    let added = ask(&engine);
    assert!(added.contains("name: \"extra\""), "{added}");
    fs::remove_dir_all(repo.join(".meada")).unwrap();
    assert_eq!(added, ask(&Engine::open(&repo).unwrap()));
    // A file that stops saying it is a template, and one that leaves.
    let engine = Engine::open(&repo).unwrap();
    fs::write(
        repo.join("cfn/extra.yaml"),
        extra.replace("AWSTemplateFormatVersion: '2010-09-09'\n", ""),
    )
    .unwrap();
    let stopped = engine.search("Extra", Some("iac"), None).unwrap();
    assert!(!stopped.contains("cfn/extra.yaml"), "{stopped}");
    fs::write(repo.join("cfn/extra.yaml"), extra).unwrap();
    assert!(
        engine
            .search("Extra", Some("iac"), None)
            .unwrap()
            .contains("cfn/extra.yaml")
    );
    fs::remove_file(repo.join("cfn/extra.yaml")).unwrap();
    let gone = engine.search("Extra", Some("iac"), None).unwrap();
    assert!(!gone.contains("cfn/extra.yaml"), "{gone}");
    fs::remove_dir_all(repo.join(".meada")).unwrap();
    assert_eq!(
        gone,
        Engine::open(&repo)
            .unwrap()
            .search("Extra", Some("iac"), None)
            .unwrap()
    );
}

/// Templates, Kubernetes objects, Compose services and Terraform share the node tables: a change
/// to one kind leaves the others' rows as they are.
#[test]
fn template_and_kubernetes_nodes_keep_each_other_when_one_changes() {
    let repo = fixture_repo("iac_cfn_kube", "cloudformation-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let queue = engine.context("OrdersQueue", None).unwrap();
    fs::create_dir_all(repo.join("k8s")).unwrap();
    let pod = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: probe\nspec:\n  containers: []\n";
    fs::write(repo.join("k8s/pod.yaml"), pod).unwrap();
    assert!(
        engine
            .search("probe", Some("iac"), None)
            .unwrap()
            .contains("Pod")
    );
    assert_eq!(engine.context("OrdersQueue", None).unwrap(), queue);
    let sam = repo.join("sam/template.yaml");
    let text = fs::read_to_string(&sam).unwrap();
    fs::write(&sam, text.replace("shop", "store")).unwrap();
    assert!(
        engine
            .context("OrdersQueue", None)
            .unwrap()
            .contains("store-orders-dev")
    );
    assert!(
        engine
            .search("probe", Some("iac"), None)
            .unwrap()
            .contains("Pod")
    );
    fs::write(repo.join("k8s/pod.yaml"), pod.replace("probe", "probe2")).unwrap();
    assert!(
        engine
            .context("OrdersQueue", None)
            .unwrap()
            .contains("store-orders-dev")
    );
    assert!(
        engine
            .search("probe2", Some("iac"), None)
            .unwrap()
            .contains("Pod")
    );
}

/// A file past the per-file size ceiling is no template (config values count it unreadable): its
/// nodes are not stored, and a small one in the same directory is.
#[test]
fn a_template_past_the_size_ceiling_adds_no_node() {
    let repo = fixture_repo("iac_cfn_big", "cloudformation-nodes", ORIGIN);
    let padding = "#".repeat(2 << 20);
    let big = format!(
        "AWSTemplateFormatVersion: '2010-09-09'\nResources:\n  Huge:\n    Type: AWS::SNS::Topic\n{padding}\n"
    );
    fs::write(repo.join("cfn/huge.yaml"), big).unwrap();
    let engine = Engine::open(&repo).unwrap();
    assert!(
        engine
            .search("Huge", Some("iac"), None)
            .unwrap()
            .contains("no matches")
    );
    assert!(
        engine
            .search("OrdersQueue", Some("iac"), None)
            .unwrap()
            .contains("sam/template.yaml")
    );
}

/// Serverless Framework (#654): the fixture `serverless-nodes` holds two services and a
/// `serverless.yml` that is none. Each function is a node `functions.<name>` with its handler and
/// events, its name `<service>-<stage>-<function>` or its own, and the `resources` section is read
/// as CloudFormation: `${self:...}` resolves, the default of `${opt:...}` resolves and is
/// overridable, `${env:...}` and `${ssm:...}` are unresolved parts.
#[test]
fn serverless_functions_are_nodes_with_handler_events_and_resources_beside_them() {
    let repo = fixture_repo("iac_sls", "serverless-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let search = answers(
        &[
            "Serverless::Function",
            "shop-dev",
            "mailer",
            "AWS::SQS::Queue",
            "ignored",
            "nothing-here",
        ],
        |q| engine.search(q, Some("iac"), None).unwrap(),
    );
    assert_golden("serverless-nodes.search.txt", &search);
    let questions = [
        "functions.createOrder",
        "functions.processOrder",
        "functions.audit",
        "functions.send",
        "OrdersQueue",
        "OrdersTable",
        "DeadLetter",
        "Assets",
        "Vault",
    ];
    let context = answers(&questions, |q| engine.context(q, None).unwrap());
    assert_golden("serverless-nodes.context.txt", &context);
    let down = |q: &str| engine.trace(Start::From(q), Direction::Down, None).unwrap();
    let up = |q: &str| engine.trace(Start::From(q), Direction::Up, None).unwrap();
    let trace = [
        answers(&["functions.createOrder", "functions.processOrder"], down),
        answers(&["OrdersQueue", "OrdersTable"], up),
    ];
    assert_golden("serverless-nodes.trace.txt", &trace.join("\n"));
    // A `serverless.yml` with no `service` is no service.
    let none = engine.search("ignored", Some("iac"), None).unwrap();
    assert!(!none.contains("misc/"), "{none}");
}

/// A change to a service file changes its nodes at the next query, as a cold index gives them; a
/// service that stops being one, and one that leaves, do the same, and a service no change
/// reached keeps its stored rows (one rewritten by hand survives).
#[test]
fn a_changed_service_is_evaluated_again_alone() {
    let repo = fixture_repo("iac_sls_edit", "serverless-nodes", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let ask = |e: &Engine| {
        let names = ["functions.createOrder", "functions.send", "OrdersQueue"];
        answers(&names, |q| e.context(q, None).unwrap())
    };
    assert!(ask(&engine).contains("name: \"shop-dev-createOrder\""));
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    db.execute(
        "UPDATE iac_nodes SET type = 'Serverless::Kept' WHERE address = 'functions.send'",
        [],
    )
    .unwrap();
    let api = repo.join("api/serverless.yml");
    let text = fs::read_to_string(&api).unwrap();
    fs::write(&api, text.replace("opt:stage, 'dev'", "opt:stage, 'qa'")).unwrap();
    let edited = ask(&engine);
    assert!(edited.contains("name: \"shop-qa-createOrder\""), "{edited}");
    assert!(edited.contains("shop-qa-orders"), "{edited}");
    assert!(edited.contains("type: Serverless::Kept"), "{edited}");
    fs::remove_dir_all(repo.join(".meada")).unwrap();
    let cold = Engine::open(&repo).unwrap();
    assert_eq!(
        edited.replace("Serverless::Kept", "Serverless::Function"),
        ask(&cold)
    );
    // The file stops being a service, then leaves.
    fs::write(&api, text.replace("service: shop", "other: shop")).unwrap();
    let stopped = cold.search("createOrder", Some("iac"), None).unwrap();
    assert!(!stopped.contains("api/serverless.yml"), "{stopped}");
    fs::write(&api, &text).unwrap();
    assert!(
        cold.search("createOrder", Some("iac"), None)
            .unwrap()
            .contains("api/serverless.yml")
    );
    fs::remove_file(&api).unwrap();
    let gone = cold.search("createOrder", Some("iac"), None).unwrap();
    assert!(!gone.contains("api/serverless.yml"), "{gone}");
}

/// A service file past the per-file size ceiling is no service: its nodes are not stored, and a
/// small one in the same repository is.
#[test]
fn a_service_past_the_size_ceiling_adds_no_node() {
    let repo = fixture_repo("iac_sls_big", "serverless-nodes", ORIGIN);
    let padding = "#".repeat(2 << 20);
    let big = format!(
        "service: huge\nprovider:\n  name: aws\nfunctions:\n  giant:\n    handler: a.b\n{padding}\n"
    );
    fs::create_dir_all(repo.join("big")).unwrap();
    fs::write(repo.join("big/serverless.yml"), big).unwrap();
    let engine = Engine::open(&repo).unwrap();
    assert!(
        engine
            .search("giant", Some("iac"), None)
            .unwrap()
            .contains("no matches")
    );
    assert!(
        engine
            .search("createOrder", Some("iac"), None)
            .unwrap()
            .contains("api/serverless.yml")
    );
}
