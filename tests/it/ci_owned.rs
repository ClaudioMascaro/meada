use std::fs;
use std::path::{Path, PathBuf};

use meada::engine::Engine;
use serde_json::{Value, json};

use crate::common::{assert_golden, fixture_repo};

const APP: &str = "example.com/config-use/app";
const DEPLOY: &str = ".github/workflows/deploy.yml";
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
            row(
                "resource::s3::archive",
                &step,
                "jobs.deploy.steps[3].with.target"
            ),
        ],
        "{:#}",
        export["uses"]
    );
    let provisions: Vec<(&str, &str, &str)> = (export["provisions"].as_array().unwrap().iter())
        .filter(|link| link["at"].as_str().is_some_and(|at| at.starts_with(DEPLOY)))
        .map(|link| {
            let text = |field: &str| link[field].as_str().unwrap();
            (text("contract"), text("side"), text("level"))
        })
        .collect();
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
    assert_eq!(uses(&export, "application.yml").len(), 3, "{export:#}");
    assert_eq!(uses(&export, "values.yaml").len(), 4, "{export:#}");
    let mut export = meada_core::export::parse(&export.to_string()).unwrap();
    (export.commit, export.dirty) = (None, None);
    assert_golden("ci-owned.app.export.json", &export.write().unwrap());
}

#[test]
fn a_file_no_ci_reader_owns_keeps_the_key_name_rule() {
    let (_, app) = workspace("ci_owned_controls");
    let plain = "storage:\n  bucket: archive\n";
    let generic = [
        ".github/settings.yml",
        ".github/workflows/data.yml",
        ".gitlab-ci.yml",
        "tools/plain/action.yml",
    ];
    for file in generic {
        write(&app, file, plain);
    }
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
fn a_gcs_address_in_a_pipeline_is_an_exact_use_on_its_ci_step() {
    let (_, app) = workspace("ci_owned_gcs");
    write(
        &app,
        DEPLOY,
        "on: push\njobs:\n  deploy:\n    steps:\n      - uses: other-org/upload@v1\n        with:\n          target: gs://acme-backups/releases\n",
    );
    let export = exported(&app);
    let step = format!("{APP}::{DEPLOY}::jobs.deploy.steps[0]");
    assert_eq!(
        uses(&export, DEPLOY),
        [row(
            "resource::gcs::acme-backups",
            &step,
            "jobs.deploy.steps[0].with.target"
        )]
    );
    let levels: Vec<&str> = (export["uses"].as_array().unwrap().iter())
        .filter(|row| row["site"] == step.as_str())
        .map(|row| row["level"].as_str().unwrap())
        .collect();
    assert_eq!(levels, ["exact"]);
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
    let bucket = engine
        .context("resource::s3::archive", Some(20000))
        .unwrap();
    assert!(
        bucket.contains(&format!("use {DEPLOY}#jobs.deploy.steps[1].with.target")),
        "{bucket}"
    );
    assert!(!bucket.contains("with.bucket"), "{bucket}");
}

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

#[test]
fn a_name_written_to_the_environment_file_gives_an_expression_no_literal() {
    let (_, app) = workspace("ci_owned_rebound");
    let pipeline = |line: &str| {
        format!(
            "on: push\nenv:\n  BUCKET: archive\n  OTHER: archive\njobs:\n  deploy:\n    steps:\n      - run: {line}\n      - uses: other-org/upload@v1\n        with:\n          target: s3://${{{{ env.BUCKET }}}}/releases\n          other: s3://${{{{ env.OTHER }}}}/releases\n"
        )
    };
    write(
        &app,
        DEPLOY,
        &pipeline("echo \"BUCKET=backups\" >> \"$GITHUB_ENV\""),
    );
    assert_eq!(
        uses(&exported(&app), DEPLOY),
        [row(
            "resource::s3::archive",
            &format!("{APP}::{DEPLOY}::jobs.deploy.steps[1]"),
            "jobs.deploy.steps[1].with.other"
        )]
    );
    write(
        &app,
        DEPLOY,
        &pipeline("cat ci/vars.env >> \"$GITHUB_ENV\""),
    );
    let none = uses(&exported(&app), DEPLOY);
    assert!(none.is_empty(), "{none:?}");
}

#[test]
fn a_nearer_env_entry_that_is_no_literal_hides_the_ones_above() {
    let (_, app) = workspace("ci_owned_shadowed");
    let pipeline = "\
on: push
env:
  NUMBER: v1
  FLAG: v1
  LIST: v1
  LIST2: v1
  MAP: v1
  FAR: v1
jobs:
  deploy:
    runs-on: ubuntu-latest
    env:
      NUMBER: 2
      FLAG: true
      LIST: [a]
      LIST2: v2
      MAP: {a: b}
    steps:
      - uses: other-org/upload@v1
        with:
          number: s3://cache-${{ env.NUMBER }}/x
          flag: s3://cache-${{ env.FLAG }}/x
          list: s3://cache-${{ env.LIST }}/x
          map: s3://cache-${{ env.MAP }}/x
          far: s3://cache-${{ env.FAR }}/x
  whole:
    runs-on: ubuntu-latest
    env: ${{ fromJSON(vars.ENV) }}
    steps:
      - uses: other-org/upload@v1
        with:
          opaque: s3://cache-${{ env.FAR }}/x
";
    write(&app, DEPLOY, pipeline);
    let step = format!("{APP}::{DEPLOY}::jobs.deploy.steps[0]");
    let key = |name: &str| format!("jobs.deploy.steps[0].with.{name}");
    let mut found = uses(&exported(&app), DEPLOY);
    found.sort();
    assert_eq!(
        found,
        [
            row("resource::s3::cache-*", &step, &key("flag")),
            row("resource::s3::cache-*", &step, &key("list")),
            row("resource::s3::cache-*", &step, &key("map")),
            row("resource::s3::cache-*", &step, &key("number")),
            row(
                "resource::s3::cache-*",
                &format!("{APP}::{DEPLOY}::jobs.whole.steps[0]"),
                "jobs.whole.steps[0].with.opaque"
            ),
            row("resource::s3::cache-v1", &step, &key("far")),
        ]
    );
}

#[test]
fn a_nearer_env_entry_with_no_value_hides_the_ones_above() {
    let (_, app) = workspace("ci_owned_hollow");
    let pipeline = "\
on: push
env:
  EMPTY: v1
  TILDE: v1
  LIST: v1
  MAP: v1
  NESTED: v1
  STEP: v1
  FAR: v1
jobs:
  deploy:
    runs-on: ubuntu-latest
    env:
      EMPTY:
      TILDE: ~
      LIST: []
      MAP: {}
      NESTED: [{a: }, []]
      STEP: v2
    steps:
      - uses: other-org/upload@v1
        env:
          STEP:
        with:
          empty: s3://cache-${{ env.EMPTY }}/x
          tilde: s3://cache-${{ env.TILDE }}/x
          list: s3://cache-${{ env.LIST }}/x
          map: s3://cache-${{ env.MAP }}/x
          nested: s3://cache-${{ env.NESTED }}/x
          step: s3://cache-${{ env.STEP }}/x
          far: s3://cache-${{ env.FAR }}/x
";
    write(&app, DEPLOY, pipeline);
    let step = format!("{APP}::{DEPLOY}::jobs.deploy.steps[0]");
    let key = |name: &str| format!("jobs.deploy.steps[0].with.{name}");
    let mut found = uses(&exported(&app), DEPLOY);
    found.sort();
    let hidden = ["empty", "list", "map", "nested", "step", "tilde"];
    let mut expected: Vec<_> = (hidden.iter())
        .map(|name| row("resource::s3::cache-*", &step, &key(name)))
        .collect();
    expected.push(row("resource::s3::cache-v1", &step, &key("far")));
    assert_eq!(found, expected);
}

#[test]
fn an_action_reads_no_env_at_its_root() {
    let (_, app) = workspace("ci_owned_action_env");
    let action = "tools/deploy/action.yml";
    write(
        &app,
        action,
        "\
env:
  NAME: archive
runs:
  using: composite
  steps:
    - uses: other-org/upload@v1
      env:
        OWN: orders
      with:
        root: s3://${{ env.NAME }}/x
        step: s3://${{ env.OWN }}/x
",
    );
    assert_eq!(
        uses(&exported(&app), action),
        [row(
            "resource::s3::orders",
            &format!("{APP}::{action}::runs.steps[0]"),
            "runs.steps[0].with.step"
        )]
    );
}
