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

/// The nodes and their references stay out of `meada export`: the export of the fixture names
/// no address.
#[test]
fn iac_nodes_stay_out_of_the_export() {
    let repo = fixture_repo("iac_export", "terraform-nodes", ORIGIN);
    let text = Engine::open(&repo).unwrap().export().unwrap();
    for address in [
        "aws_sns_topic.orders",
        "module.orders",
        "aws_sqs_queue.dlq",
        "iac",
    ] {
        assert!(!text.contains(address), "{address} in {text}");
    }
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
