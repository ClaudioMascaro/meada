//! The files the CI reader owns (#828, ADR 0028). In a pipeline or an action, a key of `with`
//! belongs to the action's vocabulary: a plain name under a typed key gives no use. A
//! structured identifier keeps its `exact` use and sits on the CI step or CI job that holds
//! its key, and a `${{ }}` expression resolves only where a literal `env` gives its value.

use std::fs;
use std::path::{Path, PathBuf};

use meada::engine::Engine;
use serde_json::{Value, json};

use crate::common::{assert_golden, fixture_repo};

const APP: &str = "example.com/config-use/app";
const DEPLOY: &str = ".github/workflows/deploy.yml";
/// One CI step per rule: typed keys (AWS, GCP and Azure aliases), a structured identifier, a
/// role ARN, and the expressions.
const PIPELINE: &str = "\
name: deploy
on: push
env:
  BUCKET: archive
  DYNAMIC: ${{ vars.BUCKET }}
jobs:
  deploy:
    runs-on: ubuntu-latest
    env:
      QUEUE_URL: https://sqs.eu-west-1.amazonaws.com/123456789012/orders
    steps:
      - uses: other-org/upload@v1
        with:
          bucket: archive
          queue: orders
          topic: events
          table: orders
          gcs_bucket: archive
          pubsub_topic: events
          servicebus_queue: orders
      - uses: other-org/upload@v1
        with:
          target: s3://archive/releases
      - uses: other-org/credentials@v1
        with:
          role-to-assume: arn:aws:iam::123456789012:role/acme-deploy
      - uses: other-org/upload@v1
        env:
          TOPIC: events
        with:
          target: s3://${{ env.BUCKET }}/releases
          notify: arn:aws:sns:eu-west-1:123456789012:${{env.TOPIC}}
          dynamic: s3://${{ env.DYNAMIC }}/releases
          missing: s3://${{ env.NOWHERE }}/releases
          whole: s3://${{ vars.BUCKET }}/releases
          staged: s3://acme-${{ vars.STAGE }}-releases/x
";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The config use fixtures (#661): `infra` provisions the queue `orders`, the topic `events`,
/// the bucket `archive` and the table `orders`; `app` names them in config files.
fn workspace(case: &str) -> (PathBuf, PathBuf) {
    let origin = |name: &str| format!("https://example.com/config-use/{name}.git");
    let infra = fixture_repo(case, "config-use-infra", Some(&origin("infra")));
    let app = fixture_repo(case, "config-use-app", Some(&origin("app")));
    (infra, app)
}

fn exported(root: &Path) -> Value {
    let text = Engine::open(root).unwrap().export().unwrap();
    serde_json::from_str(&text).unwrap()
}

/// The uses of `export` read from `file`, as (contract, site, key path).
fn uses(export: &Value, file: &str) -> Vec<(String, String, String)> {
    let text = |row: &Value, field: &str| row[field].as_str().unwrap().to_string();
    let rows = export["uses"].as_array().into_iter().flatten();
    rows.filter_map(|row| {
        let at = text(row, "at");
        let key = at.strip_prefix(file)?.strip_prefix('#')?;
        Some((text(row, "contract"), text(row, "site"), key.to_string()))
    })
    .collect()
}

fn row(contract: &str, site: &str, key: &str) -> (String, String, String) {
    (contract.into(), site.into(), key.into())
}

#[test]
fn a_pipeline_gives_no_use_by_key_name_and_an_identifier_sits_on_its_ci_node() {
    let (infra, app) = workspace("ci_owned_pipeline");
    write(&app, DEPLOY, PIPELINE);
    write(
        &infra,
        "releases.tf",
        "resource \"aws_s3_bucket\" \"releases\" { bucket = \"acme-prod-releases\" }\n",
    );
    let export = exported(&app);
    let node = |address: &str| format!("{APP}::{DEPLOY}::{address}");
    let step = node("jobs.deploy.steps[3]");
    // No row for a typed key of `with`, for the role ARN, or for a name no literal gives.
    assert_eq!(
        uses(&export, DEPLOY),
        [
            row(
                "channel::sns::events",
                &step,
                "jobs.deploy.steps[3].with.notify"
            ),
            row(
                "channel::sqs::orders",
                &node("jobs.deploy"),
                "jobs.deploy.env.QUEUE_URL"
            ),
            // An unresolved part of a name is a wildcard, and the name a pattern.
            row(
                "resource::s3::acme-*-releases",
                &step,
                "jobs.deploy.steps[3].with.staged"
            ),
            row(
                "resource::s3::archive",
                &node("jobs.deploy.steps[1]"),
                "jobs.deploy.steps[1].with.target"
            ),
            // `${{ env.BUCKET }}`, through the pipeline's literal `env`.
            row(
                "resource::s3::archive",
                &step,
                "jobs.deploy.steps[3].with.target"
            ),
        ],
        "{:#}",
        export["uses"]
    );
    // The provisioning links follow their use to its CI node, at the level of the identifier.
    let provisions: Vec<(&str, &str, &str)> = (export["provisions"].as_array().unwrap().iter())
        .filter(|link| link["at"].as_str().is_some_and(|at| at.starts_with(DEPLOY)))
        .map(|link| {
            let text = |field: &str| link[field].as_str().unwrap();
            (text("contract"), text("side"), text("level"))
        })
        .collect();
    // In the export's order: by side, then by provisioner.
    assert_eq!(
        provisions,
        [
            (
                "channel::sqs::orders",
                node("jobs.deploy").as_str(),
                "exact"
            ),
            (
                "resource::s3::archive",
                node("jobs.deploy.steps[1]").as_str(),
                "exact"
            ),
            ("resource::s3::archive", step.as_str(), "exact"),
            ("channel::sns::events", step.as_str(), "exact"),
            ("resource::s3::acme-*-releases", step.as_str(), "inferred"),
        ],
        "{:#}",
        export["provisions"]
    );
    // A CI node that holds a side is in `iac_nodes`.
    let nodes: Vec<&str> = (export["iac_nodes"].as_array().unwrap().iter())
        .map(|n| n["identity"].as_str().unwrap())
        .filter(|identity| identity.contains(DEPLOY))
        .collect();
    assert_eq!(
        nodes,
        [
            node("jobs.deploy"),
            node("jobs.deploy.steps[1]"),
            node("jobs.deploy.steps[3]")
        ]
    );
    // The other files of the repository answer as before.
    assert_eq!(uses(&export, "application.yml").len(), 3, "{export:#}");
    assert_eq!(uses(&export, "values.yaml").len(), 4, "{export:#}");
    let mut export = meada_core::export::parse(&export.to_string()).unwrap();
    (export.commit, export.dirty) = (None, None);
    assert_golden("ci-owned.app.export.json", &export.write().unwrap());
}

/// What decides is the reader's recognition of the file, never its name or its folder: a file
/// that is no pipeline and no action keeps the key-name rule, on the repository.
#[test]
fn a_file_no_ci_reader_owns_keeps_the_key_name_rule() {
    let (_, app) = workspace("ci_owned_controls");
    let plain = "storage:\n  bucket: archive\n";
    // Under `.github/`, under `.github/workflows/` with no `jobs`, another CI system, and an
    // `action.yml` with no `runs`.
    let generic = [
        ".github/settings.yml",
        ".github/workflows/data.yml",
        ".gitlab-ci.yml",
        "tools/plain/action.yml",
    ];
    for file in generic {
        write(&app, file, plain);
    }
    // An action is owned as a pipeline is.
    let action = "tools/deploy/action.yml";
    write(
        &app,
        action,
        "runs:\n  using: composite\n  steps:\n    - uses: other-org/upload@v1\n      with:\n        bucket: archive\n        target: s3://archive/x\n",
    );
    let export = exported(&app);
    for file in generic {
        assert_eq!(
            uses(&export, file),
            [row("resource::s3::archive", APP, "storage.bucket")],
            "{file}"
        );
    }
    assert_eq!(
        uses(&export, action),
        [row(
            "resource::s3::archive",
            &format!("{APP}::{action}::runs.steps[0]"),
            "runs.steps[0].with.target"
        )]
    );
    let levels: Vec<(&str, &str)> = (export["provisions"].as_array().unwrap().iter())
        .filter(|link| link["at"].as_str().is_some_and(|at| at.starts_with(".git")))
        .map(|link| {
            (
                link["side"].as_str().unwrap(),
                link["level"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(levels, [(APP, "inferred"); 3], "{export:#}");
}

#[test]
fn every_scalar_of_a_pipeline_stays_a_config_value() {
    let (_, app) = workspace("ci_owned_values");
    write(&app, DEPLOY, PIPELINE);
    let engine = Engine::open(&app).unwrap();
    for (query, key) in [
        ("archive", "jobs.deploy.steps[0].with.bucket"),
        (
            "acme-deploy",
            "jobs.deploy.steps[2].with[\"role-to-assume\"]",
        ),
        ("vars.STAGE", "jobs.deploy.steps[3].with.staged"),
    ] {
        let found = engine.search(query, Some("value"), Some(20000)).unwrap();
        assert!(found.contains(&format!("{DEPLOY}:")), "{found}");
        assert!(found.contains(key), "{query}: {found}");
    }
    // The bucket's own answer names each use by its key, the removed one gone.
    let bucket = engine
        .context("resource::s3::archive", Some(20000))
        .unwrap();
    assert!(
        bucket.contains(&format!("use {DEPLOY}#jobs.deploy.steps[1].with.target")),
        "{bucket}"
    );
    assert!(!bucket.contains("with.bucket"), "{bucket}");
}

/// ADR 0005's rule on environment variables: `vars.X` and `secrets.X` have no value in a
/// repository, so two repositories that write the same one share nothing.
#[test]
fn a_variable_is_never_matched_by_name_across_repositories() {
    let (infra, app) = workspace("ci_owned_variables");
    let other = fixture_repo(
        "ci_owned_variables",
        "config-use-infra",
        Some("https://example.com/config-use/other.git"),
    );
    fs::remove_file(other.join("main.tf")).unwrap();
    let peers = "[[peer]]\nrepo = \"example.com/config-use/infra\"\n[[peer]]\nrepo = \"example.com/config-use/app\"\n[[peer]]\nrepo = \"example.com/config-use/other\"\n";
    let pipeline = "\
on: push
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: other-org/upload@v1
        with:
          target: s3://${{ vars.BUCKET }}/releases
          queue-url: ${{ vars.QUEUE_URL }}
          other: s3://${{ secrets.BUCKET }}/releases
";
    for root in [&infra, &app, &other] {
        write(root, "meada.toml", peers);
    }
    for root in [&app, &other] {
        write(root, DEPLOY, pipeline);
    }
    for root in [&app, &other] {
        let export = exported(root);
        assert_eq!(uses(&export, DEPLOY), [], "{export:#}");
        let text = export.to_string();
        assert!(
            !text.contains("BUCKET") && !text.contains("QUEUE_URL"),
            "{text}"
        );
        assert_eq!(export["links"], json!([]));
        // The variable stays named where it is written.
        let engine = Engine::open(root).unwrap();
        let repository = export["repository"].as_str().unwrap();
        let step = format!("{repository}::{DEPLOY}::jobs.deploy.steps[0]");
        let context = engine.context(&step, Some(20000)).unwrap();
        assert!(
            context.contains("with.target = \"s3://${{ vars.BUCKET }}/releases\""),
            "{context}"
        );
    }
}

/// A pattern that fits two buckets is an open item at its key (ADR 0026, "Anchored
/// provisioning choices"), as a patterned use declared by hand is.
#[test]
fn a_pattern_that_fits_two_names_is_an_open_item_at_its_key() {
    let (infra, app) = workspace("ci_owned_ambiguous");
    write(&app, DEPLOY, PIPELINE);
    write(
        &infra,
        "releases.tf",
        "resource \"aws_s3_bucket\" \"prod\" { bucket = \"acme-prod-releases\" }\nresource \"aws_s3_bucket\" \"stg\" { bucket = \"acme-stg-releases\" }\n",
    );
    let engine = Engine::open(&app).unwrap();
    let items = engine.open_items().unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.path, DEPLOY);
    assert_eq!(item.line, Some(36));
    assert_eq!(
        item.expression,
        format!("resource::s3::acme-*-releases at {DEPLOY}#jobs.deploy.steps[3].with.staged")
    );
    let candidates: Vec<&str> = (item.candidates.iter())
        .map(|c| c.contract.as_str())
        .collect();
    assert_eq!(
        candidates,
        [
            "resource::s3::acme-prod-releases",
            "resource::s3::acme-stg-releases"
        ]
    );
}

/// The `env` that gives a value is the nearest one above the key: the CI step's, then its CI
/// job's, then the pipeline's. An `env` entry never reads the map it is written in.
#[test]
fn an_expression_reads_the_nearest_literal_env_above_its_key() {
    let (_, app) = workspace("ci_owned_env");
    let pipeline = "\
on: push
env:
  NAME: events
  SELF: s3://${{ env.NAME }}/x
jobs:
  deploy:
    runs-on: ubuntu-latest
    env:
      NAME: orders
    steps:
      - uses: other-org/upload@v1
        env:
          NAME: archive
          OWN: s3://${{ env.NAME }}/x
        with:
          target: s3://${{ env.NAME }}/x
          open: s3://${{ env.NAME /x
      - uses: other-org/upload@v1
        with:
          target: s3://${{ env.NAME }}/x
  other:
    runs-on: ubuntu-latest
    steps:
      - uses: other-org/upload@v1
        with:
          target: arn:aws:sns:eu-west-1:123456789012:${{ env.NAME }}
";
    write(&app, DEPLOY, pipeline);
    let node = |address: &str| format!("{APP}::{DEPLOY}::{address}");
    assert_eq!(
        uses(&exported(&app), DEPLOY),
        [
            row(
                "channel::sns::events",
                &node("jobs.other.steps[0]"),
                "jobs.other.steps[0].with.target"
            ),
            row(
                "resource::s3::archive",
                &node("jobs.deploy.steps[0]"),
                "jobs.deploy.steps[0].with.target"
            ),
            // Its own map holds `NAME: archive`; the CI job's is the one above.
            row(
                "resource::s3::orders",
                &node("jobs.deploy.steps[0]"),
                "jobs.deploy.steps[0].env.OWN"
            ),
            row(
                "resource::s3::orders",
                &node("jobs.deploy.steps[1]"),
                "jobs.deploy.steps[1].with.target"
            ),
        ]
    );
}
