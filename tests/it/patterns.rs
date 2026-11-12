//! Anchored names and their human decisions (#674), observed through Engine answers.

use std::{fs, path::PathBuf};

use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

use crate::common::{assert_golden, fixture_repo, without_processes};

fn workspace(case: &str) -> (PathBuf, PathBuf) {
    let infra = fixture_repo(
        case,
        "pattern-infra",
        Some("https://example.com/pattern/infra.git"),
    );
    let app = fixture_repo(
        case,
        "pattern-app",
        Some("https://example.com/pattern/app.git"),
    );
    (infra, app)
}

#[test]
fn anchored_provisioner_candidates_are_open_and_a_human_decision_selects_one_name() {
    let (infra, app) = workspace("pattern_decisions");
    let engine = Engine::open(&infra).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.repository, "example.com/pattern/infra");
    assert_eq!(
        item.candidates
            .iter()
            .map(|c| c.contract.as_str())
            .collect::<Vec<_>>(),
        ["resource::s3::archive-prod", "resource::s3::archive-stage"]
    );
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let links = data["provisions"].as_array().unwrap();
    assert_eq!(links.len(), 3, "{text}");
    assert_eq!(
        links.iter().filter(|l| l["level"] == "ambiguous").count(),
        2
    );
    assert_eq!(links.iter().filter(|l| l["level"] == "inferred").count(), 1);
    assert_golden("patterns.open.export.json", &without_processes(&text));
    let decision = format!(
        "[[decision]]\nkind = \"ambiguous-link\"\ncall = {}\ntarget = \"example.com/pattern/app::resource::s3::archive-prod\"\nby = \"human\"\n",
        serde_json::to_string(&item.id).unwrap()
    );
    fs::write(infra.join("meada.decisions.toml"), decision).unwrap();
    let engine = Engine::open(&infra).unwrap();
    assert!(engine.open_items().unwrap().is_empty());
    assert!(
        engine
            .decision_report()
            .unwrap()
            .iter()
            .all(|r| r.stale.is_empty())
    );
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let links = data["provisions"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    assert_eq!(links.iter().filter(|l| l["level"] == "decided").count(), 1);
    assert!(
        !links
            .iter()
            .any(|l| l["contract"] == "resource::s3::archive-stage")
    );
    assert_golden("patterns.decided.export.json", &without_processes(&text));
    let context = engine
        .context("aws_s3_bucket.several", Some(12000))
        .unwrap();
    let trace = engine
        .trace(
            Start::From("aws_s3_bucket.several"),
            Direction::Down,
            Some(12000),
        )
        .unwrap();
    assert!(context.contains("decided by human"), "{context}");
    assert!(trace.contains("decided by human"), "{trace}");
    assert_golden("patterns.decided.context.txt", &context);
    assert_golden("patterns.decided.trace.txt", &trace);
    let peer = Engine::open(&app).unwrap().export().unwrap();
    assert!(!peer.contains("\"level\": \"ambiguous\""), "{peer}");
}

#[test]
fn human_choices_keep_roots_independent_and_report_renamed_targets_and_removed_sources() {
    let (infra, app) = workspace("pattern_choice_lifecycle");
    fs::create_dir(infra.join("stage")).unwrap();
    fs::write(
        infra.join("stage/main.tf"),
        "resource \"aws_s3_bucket\" \"several\" { bucket = \"archive-${terraform.workspace}\" }\n",
    )
    .unwrap();
    let engine = Engine::open(&infra).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 2);
    let id = &items[0].id;
    let (dry, err, ok) =
        crate::common::meada(&infra, &["decide", "--item", id, "--dry-run"], "1\n", |c| {
            c.env("LANG", "en_US.UTF-8");
        });
    assert!(ok, "{err}");
    assert!(
        dry.contains("archive-prod") && dry.contains("archive-stage"),
        "{dry}"
    );
    assert!(!infra.join("meada.decisions.toml").exists());
    let (out, err, ok) = crate::common::meada(&infra, &["decide", "--item", id], "1\n", |c| {
        c.env("LANG", "en_US.UTF-8");
    });
    assert!(ok, "{err}");
    assert!(out.contains("wrote 1 decision"), "{out}");
    let engine = Engine::open(&infra).unwrap();
    let remaining = engine.open_items().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_ne!(&remaining[0].id, id);
    let original = fs::read_to_string(app.join("meada.toml")).unwrap();
    fs::write(
        app.join("meada.toml"),
        original.replace("archive-prod", "archive-next"),
    )
    .unwrap();
    let engine = Engine::open(&infra).unwrap();
    let stale: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        stale["stale_decisions"].as_array().unwrap().len(),
        1,
        "{stale}"
    );
    assert_eq!(stale["stale_decisions"][0]["cause"], "target-gone");
    assert_eq!(engine.open_items().unwrap().len(), 2);
    fs::write(app.join("meada.toml"), original).unwrap();
    let engine = Engine::open(&infra).unwrap();
    assert!(
        engine
            .decision_report()
            .unwrap()
            .iter()
            .all(|r| r.stale.is_empty())
    );
    let original = fs::read_to_string(infra.join("main.tf")).unwrap();
    fs::write(
        infra.join("main.tf"),
        original
            .lines()
            .filter(|l| !l.contains("\"several\""))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let stale: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        stale["stale_decisions"].as_array().unwrap().len(),
        1,
        "{stale}"
    );
    assert_eq!(stale["stale_decisions"][0]["cause"], "call-gone");
}

#[test]
fn literal_provisioners_match_patterned_uses_and_the_use_owns_its_choice() {
    let (infra, app) = workspace("pattern_use_decision");
    fs::write(infra.join("main.tf"), "resource \"aws_s3_bucket\" \"east\" { bucket = \"receipts-east\" }\nresource \"aws_s3_bucket\" \"west\" { bucket = \"receipts-west\" }\nresource \"aws_s3_bucket\" \"single\" { bucket = \"backup-east\" }\n").unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    fs::write(app.join("meada.toml"), "[[peer]]\nrepo = \"example.com/pattern/infra\"\n[[resource]]\nkind = \"s3\"\nid = \"receipts-*\"\nrole = \"read\"\n[[resource]]\nkind = \"s3\"\nid = \"backup-*\"\nrole = \"read\"\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let links = data["provisions"]
        .as_array()
        .expect("literal provisioners match patterned uses");
    assert_eq!(links.len(), 3, "{text}");
    assert_eq!(
        links.iter().filter(|l| l["level"] == "ambiguous").count(),
        2
    );
    assert_eq!(links.iter().filter(|l| l["level"] == "inferred").count(), 1);
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].repository, "example.com/pattern/app");
    assert_eq!(items[0].expression, "resource::s3::receipts-*");
    assert_eq!(
        items[0]
            .candidates
            .iter()
            .map(|c| c.contract.as_str())
            .collect::<Vec<_>>(),
        ["resource::s3::receipts-east", "resource::s3::receipts-west"]
    );
    assert_golden("patterns.use.open.export.json", &without_processes(&text));
    let decision = format!(
        "[[decision]]\nkind = \"ambiguous-link\"\ncall = {}\ntarget = \"example.com/pattern/infra::resource::s3::receipts-west\"\nby = \"human\"\n",
        serde_json::to_string(&items[0].id).unwrap()
    );
    fs::write(app.join("meada.decisions.toml"), decision).unwrap();
    let engine = Engine::open(&app).unwrap();
    assert!(engine.open_items().unwrap().is_empty());
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let links = data["provisions"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    let picked = links.iter().find(|l| l["level"] == "decided").unwrap();
    assert!(
        picked["provisioner"]
            .as_str()
            .unwrap()
            .ends_with("aws_s3_bucket.west")
    );
    assert_golden(
        "patterns.use.decided.export.json",
        &without_processes(&text),
    );
}

#[test]
fn an_image_builder_pattern_with_two_literal_runner_names_is_a_builder_open_item() {
    let (infra, app) = workspace("pattern_image_builder");
    fs::write(infra.join("main.tf"), "").unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/pattern/infra\"\n",
    )
    .unwrap();
    fs::create_dir_all(app.join(".github/workflows")).unwrap();
    fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/pattern/orders-${{ matrix.region }}:v1\n").unwrap();
    fs::write(infra.join("compose.yaml"), "services:\n  east:\n    image: ghcr.io/pattern/orders-east:v1\n  west:\n    image: ghcr.io/pattern/orders-west:v1\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let links = data["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    assert!(links.iter().all(|l| l["level"] == "ambiguous"), "{text}");
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].repository, "example.com/pattern/app");
    assert_eq!(
        items[0]
            .candidates
            .iter()
            .map(|c| c.contract.as_str())
            .collect::<Vec<_>>(),
        [
            "image::ghcr.io/pattern/orders-east",
            "image::ghcr.io/pattern/orders-west"
        ]
    );
    assert_golden("patterns.image.open.export.json", &without_processes(&text));
    let (out, err, ok) = crate::common::meada(&app, &["decide"], "2\n", |c| {
        c.env("LANG", "en_US.UTF-8");
    });
    assert!(ok, "{err}");
    assert!(out.contains("wrote 1 decision"), "{out}");
    let engine = Engine::open(&app).unwrap();
    assert!(engine.open_items().unwrap().is_empty());
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(data["links"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(
        data["links"][0]["contract"],
        "image::ghcr.io/pattern/orders-west"
    );
    assert_eq!(data["links"][0]["level"], "decided");
    assert_eq!(data["links"][0]["by"], "human");
    assert_golden(
        "patterns.image.decided.export.json",
        &without_processes(&text),
    );
    let status = meada::engine::status(&infra).unwrap();
    assert!(
        status.contains("orphan image::ghcr.io/pattern/orders-east: no builder"),
        "{status}"
    );
    let context = engine.context("services.west", Some(12000)).unwrap();
    assert!(context.contains("decided by human"), "{context}");
    let compose = fs::read_to_string(infra.join("compose.yaml")).unwrap();
    fs::write(
        infra.join("compose.yaml"),
        compose.replace("orders-west", "orders-north"),
    )
    .unwrap();
    let stale: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(stale["stale_decisions"][0]["cause"], "target-gone");
    assert_eq!(engine.open_items().unwrap().len(), 1);
    fs::remove_file(app.join(".github/workflows/build.yml")).unwrap();
    let stale: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(stale["stale_decisions"][0]["cause"], "call-gone");
    assert_eq!(stale["links"], serde_json::json!([]));
}

#[test]
fn one_image_pattern_lists_thousands_of_runner_names_in_bounded_time() {
    let (infra, app) = workspace("pattern_candidate_growth");
    fs::write(infra.join("main.tf"), "").unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/pattern/infra\"\n",
    )
    .unwrap();
    fs::create_dir_all(app.join(".github/workflows")).unwrap();
    fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/bounded/order-${{ matrix.region }}\n").unwrap();
    for n in 0..2000 {
        let directory = infra.join(format!("chart-{n}"));
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("values.yaml"),
            format!("image:\n  repository: ghcr.io/bounded/order-{n}\n"),
        )
        .unwrap();
    }
    let engine = Engine::open(&infra).unwrap();
    let started = std::time::Instant::now();
    let items = engine.open_items().unwrap();
    let elapsed = started.elapsed();
    eprintln!("2000 runner names: {elapsed:?}");
    assert!(elapsed < std::time::Duration::from_secs(30), "{elapsed:?}");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].candidates.len(), 2000);
    let expected: std::collections::BTreeSet<_> = (0..2000)
        .map(|n| format!("image::ghcr.io/bounded/order-{n}"))
        .collect();
    assert_eq!(
        items[0]
            .candidates
            .iter()
            .map(|c| c.contract.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
}

#[test]
fn one_use_pattern_lists_thousands_of_literal_provisioners_in_bounded_time() {
    let (infra, app) = workspace("pattern_provision_growth");
    let source = (0..5000)
        .map(|n| {
            format!("resource \"aws_s3_bucket\" \"bucket{n}\" {{ bucket = \"receipts-{n}\" }}\n")
        })
        .collect::<String>();
    fs::write(infra.join("main.tf"), source).unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    fs::write(app.join("meada.toml"), "[[peer]]\nrepo = \"example.com/pattern/infra\"\n[[resource]]\nkind = \"s3\"\nid = \"receipts-*\"\nrole = \"read\"\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    let started = std::time::Instant::now();
    let items = engine.open_items().unwrap();
    let elapsed = started.elapsed();
    eprintln!("5000 provisioner names: {elapsed:?}");
    assert!(elapsed < std::time::Duration::from_secs(30), "{elapsed:?}");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].candidates.len(), 5000);
    assert!(
        items[0]
            .candidates
            .iter()
            .all(|c| c.repository == "example.com/pattern/infra"
                && c.contract.starts_with("resource::s3::receipts-"))
    );
}

#[test]
fn a_literal_asterisk_in_a_provisioner_is_not_unresolved_name_evidence() {
    let (infra, app) = workspace("pattern_literal_star");
    fs::write(
        infra.join("main.tf"),
        "resource \"aws_s3_bucket\" \"literal\" { bucket = \"archive-*\" }\n",
    )
    .unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    let engine = Engine::open(&infra).unwrap();
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    assert!(data.get("provisions").is_none(), "{text}");
    assert!(engine.open_items().unwrap().is_empty());
}

fn two_image_builders(case: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (infra, app) = workspace(case);
    let other = fixture_repo(
        case,
        "image-app",
        Some("https://example.com/pattern/other.git"),
    );
    fs::write(infra.join("main.tf"), "").unwrap();
    fs::write(infra.join("meada.toml"), "[[peer]]\nrepo = \"example.com/pattern/app\"\n[[peer]]\nrepo = \"example.com/pattern/other\"\n").unwrap();
    fs::write(infra.join("compose.yaml"), "services:\n  east:\n    image: ghcr.io/pattern/orders-east\n  west:\n    image: ghcr.io/pattern/orders-west\n").unwrap();
    fs::remove_file(other.join("compose.yaml")).unwrap();
    for repo in [&app, &other] {
        fs::write(
            repo.join("meada.toml"),
            "[[peer]]\nrepo = \"example.com/pattern/infra\"\n",
        )
        .unwrap();
        fs::create_dir_all(repo.join(".github/workflows")).unwrap();
        fs::write(repo.join(".github/workflows/build.yml"), "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/pattern/orders-${{ matrix.region }}\n").unwrap();
    }
    (infra, app, other)
}

fn choose(repo: &std::path::Path, id: &str, target: &str) {
    fs::write(
        repo.join("meada.decisions.toml"),
        format!(
            "[[decision]]\nkind = \"ambiguous-link\"\ncall = {}\ntarget = {}\nby = \"human\"\n",
            serde_json::to_string(id).unwrap(),
            serde_json::to_string(target).unwrap()
        ),
    )
    .unwrap();
}

fn image_links(data: &Value) -> std::collections::BTreeSet<(&str, &str, &str, &str)> {
    data["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|link| {
            (
                link["consumer"].as_str().unwrap(),
                link["provider"].as_str().unwrap(),
                link["contract"].as_str().unwrap(),
                link["level"].as_str().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_rejected_builder_name_does_not_create_a_second_runner_choice() {
    let (infra, app, _) = two_image_builders("pattern_mixed_image_choices");
    let engine = Engine::open(&infra).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 4);
    let runner = items
        .iter()
        .find(|i| i.id.contains("services.west"))
        .unwrap();
    let other = runner
        .candidates
        .iter()
        .find(|c| c.repository == "example.com/pattern/other")
        .unwrap();
    choose(
        &infra,
        &runner.id,
        &format!("{}::{}", other.repository, other.contract),
    );
    let before: Value =
        serde_json::from_str(&Engine::open(&infra).unwrap().export().unwrap()).unwrap();
    assert_eq!(before["links"].as_array().unwrap().len(), 3);
    assert!(before.get("stale_decisions").is_none(), "{before}");
    assert!(
        before["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["contract"] == "image::ghcr.io/pattern/orders-west"
                && l["provider"] == "example.com/pattern/other"
                && l["level"] == "decided")
    );
    let builder = items
        .iter()
        .find(|i| i.repository == "example.com/pattern/app")
        .unwrap();
    fs::write(app.join("meada.decisions.toml"), format!("[[decision]]\nkind = \"ambiguous-link\"\ncall = {}\ntarget = \"example.com/pattern/infra::image::ghcr.io/pattern/orders-east\"\nby = \"human\"\n", serde_json::to_string(&builder.id).unwrap())).unwrap();
    let engine = Engine::open(&infra).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(
        items.len(),
        2,
        "{:?}",
        items
            .iter()
            .map(|i| (&i.id, i.candidates.len()))
            .collect::<Vec<_>>()
    );
    assert!(items.iter().all(|i| i.candidates.len() == 2));
    assert_eq!(
        items
            .iter()
            .map(|i| i.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "example.com/pattern/infra::compose.yaml::services.east::image::ghcr.io/pattern/orders-east (root .)",
            "image-builder::example.com/pattern/other::image::ghcr.io/pattern/orders-*",
        ]
        .into_iter()
        .collect()
    );
    let after: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        after["stale_decisions"].as_array().map(Vec::len),
        Some(1),
        "{after}"
    );
    assert_eq!(after["stale_decisions"][0]["cause"], "item-not-open");
    assert_eq!(after["links"].as_array().unwrap().len(), 3);
    assert_eq!(
        image_links(&after),
        [
            (
                "example.com/pattern/infra::compose.yaml::services.east",
                "example.com/pattern/app",
                "image::ghcr.io/pattern/orders-east",
                "ambiguous",
            ),
            (
                "example.com/pattern/infra::compose.yaml::services.east",
                "example.com/pattern/other",
                "image::ghcr.io/pattern/orders-east",
                "ambiguous",
            ),
            (
                "example.com/pattern/infra::compose.yaml::services.west",
                "example.com/pattern/other",
                "image::ghcr.io/pattern/orders-west",
                "ambiguous",
            ),
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn an_inactive_builder_pattern_is_not_a_candidate_through_its_repositorys_literal() {
    let (infra, app, other) = two_image_builders("pattern_active_provider_choices");
    fs::write(app.join(".github/workflows/build.yml"), "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: |\n            ghcr.io/pattern/orders-${{ matrix.region }}\n            ghcr.io/pattern/orders-west\n").unwrap();
    fs::write(other.join(".github/workflows/build.yml"), "jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/pattern/orders-west\n").unwrap();
    let engine = Engine::open(&infra).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 2);
    let runner = items
        .iter()
        .find(|i| i.id.contains("services.west"))
        .unwrap();
    assert_eq!(runner.candidates.len(), 3);
    let pattern = runner
        .candidates
        .iter()
        .find(|c| c.contract.contains('*'))
        .unwrap();
    choose(
        &infra,
        &runner.id,
        &format!("{}::{}", pattern.repository, pattern.contract),
    );
    let before: Value =
        serde_json::from_str(&Engine::open(&infra).unwrap().export().unwrap()).unwrap();
    assert!(before.get("stale_decisions").is_none(), "{before}");
    assert_eq!(before["links"].as_array().unwrap().len(), 2);
    assert!(
        before["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["contract"] == "image::ghcr.io/pattern/orders-west"
                && l["level"] == "decided")
    );
    let builder = items
        .iter()
        .find(|i| i.repository == "example.com/pattern/app")
        .unwrap();
    choose(
        &app,
        &builder.id,
        "example.com/pattern/infra::image::ghcr.io/pattern/orders-east",
    );
    let engine = Engine::open(&infra).unwrap();
    let item = engine.local_item(&runner.id).unwrap().unwrap();
    assert_eq!(
        item.candidates
            .iter()
            .map(|c| (c.repository.as_str(), c.contract.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                "example.com/pattern/app",
                "image::ghcr.io/pattern/orders-west"
            ),
            (
                "example.com/pattern/other",
                "image::ghcr.io/pattern/orders-west"
            )
        ]
    );
    assert_eq!(engine.open_items().unwrap().len(), 1);
    let after: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(
        after["stale_decisions"].as_array().map(Vec::len),
        Some(1),
        "{after}"
    );
    assert_eq!(after["stale_decisions"][0]["cause"], "not-candidate");
    let links = after["links"].as_array().unwrap();
    assert_eq!(links.len(), 3);
    assert_eq!(
        image_links(&after),
        [
            (
                "example.com/pattern/infra::compose.yaml::services.east",
                "example.com/pattern/app",
                "image::ghcr.io/pattern/orders-east",
                "decided",
            ),
            (
                "example.com/pattern/infra::compose.yaml::services.west",
                "example.com/pattern/app",
                "image::ghcr.io/pattern/orders-west",
                "ambiguous",
            ),
            (
                "example.com/pattern/infra::compose.yaml::services.west",
                "example.com/pattern/other",
                "image::ghcr.io/pattern/orders-west",
                "ambiguous",
            ),
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn patterned_sdk_use_items_keep_their_call_location_in_partial_and_full_queries() {
    let (infra, app) = workspace("pattern_use_locations");
    fs::write(infra.join("main.tf"), "resource \"aws_s3_bucket\" \"east\" { bucket = \"receipts-east\" }\nresource \"aws_s3_bucket\" \"west\" { bucket = \"receipts-west\" }\n").unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    fs::write(
        app.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/pattern/infra\"\n",
    )
    .unwrap();
    fs::write(app.join("read.py"), "import boto3\ns3 = boto3.client('s3')\ndef receive():\n    return s3.get_object(Bucket='receipts-*', Key='x')\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    let read = || {
        engine
            .open_items()
            .unwrap()
            .into_iter()
            .map(|i| {
                (
                    i.id,
                    i.path,
                    i.line,
                    i.expression,
                    i.candidates
                        .into_iter()
                        .map(|c| (c.repository, c.contract, c.site))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };
    let partial = read();
    assert_eq!(
        partial,
        meada::engine::with_every_row(read),
        "partial and full queries place the same source item"
    );
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].path, "read.py");
    assert_eq!(items[0].line, Some(4));
    assert!(items[0].expression.contains("get_object"));
}

#[test]
fn a_patterned_use_declared_at_a_config_key_keeps_its_location_and_human_choice() {
    let (infra, app) = workspace("pattern_config_key_choice");
    fs::write(infra.join("main.tf"), "resource \"aws_s3_bucket\" \"east\" { bucket = \"receipts-east\" }\nresource \"aws_s3_bucket\" \"west\" { bucket = \"receipts-west\" }\n").unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    fs::write(app.join("meada.toml"), "[[peer]]\nrepo = \"example.com/pattern/infra\"\n[[resource]]\nkind = \"s3\"\nid = \"receipts-*\"\nrole = \"read\"\nvia = [\"settings.yaml#data.bucket\"]\n").unwrap();
    fs::write(
        app.join("settings.yaml"),
        "data:\n  bucket: receipts-*\n  other: keep\n",
    )
    .unwrap();
    let engine = Engine::open(&app).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert!(item.id.contains("::settings.yaml#data.bucket::"));
    assert_eq!(item.path, "settings.yaml");
    assert_eq!(item.line, Some(2));
    assert_eq!(
        item.expression,
        "resource::s3::receipts-* at settings.yaml#data.bucket"
    );
    assert_eq!(
        item.candidates
            .iter()
            .map(|c| c.contract.as_str())
            .collect::<Vec<_>>(),
        ["resource::s3::receipts-east", "resource::s3::receipts-west"]
    );
    fs::write(app.join("meada.decisions.toml"), format!("[[decision]]\nkind = \"ambiguous-link\"\ncall = {}\ntarget = \"example.com/pattern/infra::resource::s3::receipts-west\"\nby = \"human\"\n", serde_json::to_string(&item.id).unwrap())).unwrap();
    let engine = Engine::open(&app).unwrap();
    assert!(engine.open_items().unwrap().is_empty());
    let text = engine.export().unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let links = data["provisions"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{text}");
    assert_eq!(
        links[0]["side"],
        "example.com/pattern/app::settings.yaml#data.bucket"
    );
    assert_eq!(links[0]["level"], "decided");
    assert!(
        links[0]["provisioner"]
            .as_str()
            .unwrap()
            .ends_with("aws_s3_bucket.west")
    );
}

#[test]
fn config_key_pattern_items_and_model_previews_keep_redacted_and_cut_payloads_out() {
    let (infra, app) = workspace("pattern_config_key_payloads");
    fs::write(infra.join("main.tf"), "resource \"aws_s3_bucket\" \"east\" { bucket = \"receipts-east\" }\nresource \"aws_s3_bucket\" \"west\" { bucket = \"receipts-west\" }\n").unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    let secret = "pattern-secret-never-send";
    // The ordinary metadata key shares a physical JSON line with the Secret scalar.
    fs::write(
        app.join("private.json"),
        serde_json::to_string(&serde_json::json!({
            "apiVersion": "v1", "kind": "Secret",
            "metadata": {"name": "credentials"}, "stringData": {"api_key": secret}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        app.join("cut.yaml"),
        format!("bucket: cut-visible-{}CUT-HIDDEN-TAIL\n", "x".repeat(1100)),
    )
    .unwrap();
    fs::write(app.join("meada.toml"), "[[peer]]\nrepo = \"example.com/pattern/infra\"\n[[resource]]\nkind = \"s3\"\nid = \"receipts-*\"\nrole = \"read\"\nvia = [\"private.json#stringData.api_key\", \"private.json#metadata.name\", \"cut.yaml#bucket\"]\n").unwrap();
    let engine = Engine::open(&app).unwrap();
    assert!(
        engine
            .search("api_key", Some("value"), Some(12000))
            .unwrap()
            .contains("redacted")
    );
    assert!(
        engine
            .search("bucket", Some("value"), Some(12000))
            .unwrap()
            .contains("(cut)")
    );
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|i| i.candidates.len() == 2));
    let (request, err, ok) =
        crate::common::meada(&app, &["decide", "--decider", "--show-request"], "", |c| {
            c.env("LANG", "en_US.UTF-8")
                .env("MEADA_DECIDER_PROTOCOL", "chat")
                .env("MEADA_DECIDER_URL", "http://127.0.0.1:1/v1")
                .env("MEADA_DECIDER_MODEL", "local/pattern-control")
                .env_remove("MEADA_DECIDER_KEY");
        });
    assert!(ok, "{err}");
    assert_eq!(
        request
            .matches("POST http://127.0.0.1:1/v1/chat/completions")
            .count(),
        3,
        "{request}"
    );
    for payload in [secret, "cut-visible-", "CUT-HIDDEN-TAIL"] {
        assert!(!request.contains(payload), "{payload} in model preview");
        assert!(items.iter().all(|i| !i.expression.contains(payload)));
    }
    assert!(!app.join("meada.decisions.toml").exists());
    assert_eq!(
        items
            .iter()
            .map(|i| (i.path.as_str(), i.line, i.expression.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                "cut.yaml",
                Some(1),
                "resource::s3::receipts-* at cut.yaml#bucket"
            ),
            (
                "private.json",
                Some(1),
                "resource::s3::receipts-* at private.json#metadata.name"
            ),
            (
                "private.json",
                Some(1),
                "resource::s3::receipts-* at private.json#stringData.api_key"
            ),
        ]
    );
}

#[test]
fn many_config_key_sources_keep_indexed_locations_in_bounded_time() {
    let (infra, app) = workspace("pattern_config_source_growth");
    fs::write(infra.join("main.tf"), "resource \"aws_s3_bucket\" \"east\" { bucket = \"receipts-east\" }\nresource \"aws_s3_bucket\" \"west\" { bucket = \"receipts-west\" }\n").unwrap();
    fs::remove_file(app.join("app.ts")).unwrap();
    // 10,000 sources in 20 files of 500 entries: each file holds 1,000 values, the most a
    // generic config file keeps (#817).
    let (files, per_file) = (20, 500);
    let path = |f: usize| format!("settings-{f:02}.json");
    let via: Vec<String> = (0..files)
        .map(|f| format!("\"{}#entries[enabled=true].bucket\"", path(f)))
        .collect();
    fs::write(app.join("meada.toml"), format!("[[peer]]\nrepo = \"example.com/pattern/infra\"\n[[resource]]\nkind = \"s3\"\nid = \"receipts-*\"\nrole = \"read\"\nvia = [{}]\n", via.join(", "))).unwrap();
    let entries: Vec<_> = (0..per_file)
        .map(|_| serde_json::json!({"enabled": true, "bucket": "receipts-*"}))
        .collect();
    let settings = serde_json::to_string(&serde_json::json!({"entries": entries})).unwrap();
    for f in 0..files {
        fs::write(app.join(path(f)), &settings).unwrap();
    }
    let engine = Engine::open(&app).unwrap();
    let start = std::time::Instant::now();
    let items = engine.open_items().unwrap();
    let elapsed = start.elapsed();
    eprintln!("10000 config-key sources: {elapsed:?}");
    assert!(elapsed < std::time::Duration::from_secs(30), "{elapsed:?}");
    assert_eq!(items.len(), 10000);
    let expected: std::collections::BTreeMap<_, _> = (0..files)
        .flat_map(|f| (0..per_file).map(move |n| (f, n)))
        .map(|(f, n)| {
            let key = format!("{}#entries[{n}].bucket", path(f));
            (
                format!("provision-use::example.com/pattern/app::{key}::resource::s3::receipts-*"),
                format!("resource::s3::receipts-* at {key}"),
            )
        })
        .collect();
    assert_eq!(
        items
            .iter()
            .map(|i| i.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        expected
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert!(items.iter().all(|i| {
        i.repository == "example.com/pattern/app"
            && i.id.contains(&format!("::{}#", i.path))
            && i.line == Some(1)
            && expected.get(&i.id) == Some(&i.expression)
    }));
    assert!(items.iter().all(|i| {
        i.candidates
            .iter()
            .map(|c| (c.repository.as_str(), c.contract.as_str(), c.site.as_str()))
            .collect::<Vec<_>>()
            == [
                (
                    "example.com/pattern/infra",
                    "resource::s3::receipts-east",
                    "example.com/pattern/infra::main.tf::aws_s3_bucket.east",
                ),
                (
                    "example.com/pattern/infra",
                    "resource::s3::receipts-west",
                    "example.com/pattern/infra::main.tf::aws_s3_bucket.west",
                ),
            ]
    }));
}
