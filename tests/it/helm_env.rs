use std::path::{Path, PathBuf};

use meada::engine::Engine;
use serde_json::Value;

use crate::common::{fixture_repo, of_files};

const CHART: &str = "svc/chart/api";

fn fixture(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "helm-env",
        Some("https://example.com/acme/helm-env.git"),
    )
}

fn export_of(engine: &Engine) -> Value {
    serde_json::from_str(&engine.export().unwrap()).unwrap()
}

fn export(case: &str, change: impl FnOnce(&Path)) -> Value {
    let repo = fixture(case);
    change(&repo);
    export_of(&Engine::open(&repo).unwrap())
}

fn write(repo: &Path, file: &str, text: &str) {
    let path = repo.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn values(envs: &[(&str, &str)]) -> String {
    let lines: String = envs
        .iter()
        .map(|(n, v)| format!("    {n}: {v}\n"))
        .collect();
    format!("lib-chart:\n  envs:\n{lines}")
}

fn compose(repo: &Path, environment: &[(&str, &str)]) {
    let lines: String = environment
        .iter()
        .map(|(n, v)| format!("      {n}: {v}\n"))
        .collect();
    write(repo, "svc/Dockerfile", "FROM scratch\nCOPY . .\n");
    let text = format!("services:\n  svc:\n    build: ./svc\n    environment:\n{lines}");
    write(repo, "docker-compose.yml", &text);
}

fn consumed(export: &Value, file: &str) -> Vec<String> {
    let at = format!("::{file}::");
    let mut found: Vec<String> = (export["consumers"].as_array().unwrap().iter())
        .filter(|c| c["site"].as_str().unwrap().contains(&at))
        .filter_map(|c| c["contract"].as_str())
        .filter(|c| c.starts_with("channel::"))
        .map(String::from)
        .collect();
    found.sort();
    found
}

fn consumer<'e>(export: &'e Value, file: &str, contract: &str) -> &'e Value {
    let at = format!("::{file}::");
    (export["consumers"].as_array().unwrap().iter())
        .find(|c| c["site"].as_str().unwrap().contains(&at) && c["contract"] == contract)
        .unwrap_or_else(|| panic!("no {contract} consumer in {file}: {export}"))
}

fn level_from(export: &Value, function: &str) -> String {
    let from = format!("::tools/writers.go::{function}::");
    let found = (export["links"].as_array().unwrap().iter())
        .find(|l| {
            l["provider"].as_str().unwrap().contains(&from)
                && l["consumer"]
                    .as_str()
                    .unwrap()
                    .contains("::svc/consumer.go::")
        })
        .unwrap_or_else(|| panic!("no link from {function}: {export}"));
    found["level"].as_str().unwrap().to_string()
}

fn helm_rows(export: &Value) -> Vec<&Value> {
    (export["executions"].as_array().into_iter().flatten())
        .filter(|x| {
            x["workload"]
                .as_str()
                .unwrap()
                .ends_with("::lib-chart.envs")
        })
        .collect()
}

#[test]
fn a_declared_chart_environment_names_the_topic() {
    let export = export("helm_env_declared", |_| {});
    assert_eq!(
        consumed(&export, "svc/consumer.go"),
        ["channel::kafka::orders.created"]
    );
    assert_eq!(level_from(&export, "Created"), "declared");
    let via = consumer(&export, "svc/consumer.go", "channel::kafka::orders.created")["via"]
        .as_str()
        .unwrap();
    assert!(via.contains("helm_env lib-chart.envs"), "{via}");
}

#[test]
fn environments_that_disagree_give_one_inferred_side_each() {
    let export = export("helm_env_disagree", |repo| {
        let prd = values(&[("ORDERS_TOPIC", "orders.v2")]);
        write(repo, &format!("{CHART}/values-prd.yaml"), &prd);
    });
    assert_eq!(
        consumed(&export, "svc/consumer.go"),
        [
            "channel::kafka::orders.created",
            "channel::kafka::orders.v2"
        ]
    );
    assert_eq!(level_from(&export, "Created"), "inferred");
    assert_eq!(level_from(&export, "V2"), "inferred");
}

#[test]
fn an_environment_lacking_the_variable_gives_inferred() {
    let export = export("helm_env_lacking", |repo| {
        let prd = values(&[("OTHER", "x")]);
        write(repo, &format!("{CHART}/values-prd.yaml"), &prd);
    });
    assert_eq!(
        consumed(&export, "svc/consumer.go"),
        ["channel::kafka::orders.created"]
    );
    assert_eq!(level_from(&export, "Created"), "inferred");
}

#[test]
fn the_base_values_file_lies_under_each_environment() {
    let export = export("helm_env_base", |repo| {
        let base = values(&[("ORDERS_TOPIC", "orders.created")]);
        write(repo, &format!("{CHART}/values.yaml"), &base);
        for env in ["stg", "prd"] {
            let other = values(&[("OTHER", env)]);
            write(repo, &format!("{CHART}/values-{env}.yaml"), &other);
        }
    });
    assert_eq!(level_from(&export, "Created"), "declared");
}

#[test]
fn a_comment_only_base_values_file_is_an_empty_map() {
    let export = export("helm_env_empty_base", |repo| {
        write(
            repo,
            &format!("{CHART}/values.yaml"),
            "# set per environment\n",
        );
    });
    assert_eq!(level_from(&export, "Created"), "declared");
}

#[test]
fn an_empty_environment_values_file_is_the_base() {
    let export = export("helm_env_empty_env", |repo| {
        let base = values(&[("ORDERS_TOPIC", "orders.created")]);
        write(repo, &format!("{CHART}/values.yaml"), &base);
        write(
            repo,
            &format!("{CHART}/values-stg.yaml"),
            &values(&[("OTHER", "x")]),
        );
        write(
            repo,
            &format!("{CHART}/values-prd.yaml"),
            "# nothing here yet\n",
        );
    });
    assert_eq!(level_from(&export, "Created"), "declared");
}

#[test]
fn a_chart_with_only_values_yaml_is_one_environment() {
    let export = export("helm_env_only_base", |repo| {
        let base = values(&[("ORDERS_TOPIC", "orders.created")]);
        write(repo, &format!("{CHART}/values.yaml"), &base);
        for env in ["stg", "prd"] {
            std::fs::remove_file(repo.join(format!("{CHART}/values-{env}.yaml"))).unwrap();
        }
    });
    assert_eq!(level_from(&export, "Created"), "declared");
    let rows = helm_rows(&export);
    assert_eq!(rows.len(), 1, "{export}");
    let workload = rows[0]["workload"].as_str().unwrap();
    assert!(workload.ends_with("::svc/chart/api/values.yaml::lib-chart.envs"));
}

#[test]
fn a_chart_that_never_sets_the_variable_says_nothing() {
    let export = export("helm_env_silent_chart", |repo| {
        write(
            repo,
            "svc/chart/worker/Chart.yaml",
            "apiVersion: v2\nname: worker\n",
        );
        let other = values(&[("OTHER", "y")]);
        write(repo, "svc/chart/worker/values-stg.yaml", &other);
    });
    assert_eq!(level_from(&export, "Created"), "declared");
}

#[test]
fn two_charts_that_disagree_give_inferred() {
    let export = export("helm_env_two_charts", |repo| {
        write(
            repo,
            "svc/chart/worker/Chart.yaml",
            "apiVersion: v2\nname: worker\n",
        );
        let v2 = values(&[("ORDERS_TOPIC", "orders.v2")]);
        write(repo, "svc/chart/worker/values.yaml", &v2);
    });
    assert_eq!(level_from(&export, "Created"), "inferred");
    assert_eq!(level_from(&export, "V2"), "inferred");
}

#[test]
fn a_compose_value_equal_to_the_declared_one_joins_it() {
    let export = export("helm_env_compose_same", |repo| {
        compose(repo, &[("ORDERS_TOPIC", "orders.created")]);
    });
    assert_eq!(
        consumed(&export, "svc/consumer.go"),
        ["channel::kafka::orders.created"]
    );
    let via = consumer(&export, "svc/consumer.go", "channel::kafka::orders.created")["via"]
        .as_str()
        .unwrap();
    assert!(via.contains("helm_env lib-chart.envs"), "{via}");
    assert!(via.contains("docker-compose.yml"), "{via}");
    assert_eq!(level_from(&export, "Created"), "declared");
}

#[test]
fn a_different_compose_value_is_its_own_inferred_side() {
    let export = export("helm_env_compose_other", |repo| {
        compose(repo, &[("ORDERS_TOPIC", "orders.v2")]);
    });
    assert_eq!(level_from(&export, "Created"), "declared");
    assert_eq!(level_from(&export, "V2"), "inferred");
}

#[test]
fn the_export_runs_the_package_from_each_environment() {
    let export = export("helm_env_rows", |_| {});
    let rows = helm_rows(&export);
    let workloads: Vec<&str> = rows
        .iter()
        .map(|x| x["workload"].as_str().unwrap())
        .collect();
    assert_eq!(workloads.len(), 2, "{export}");
    for (row, env) in rows.iter().zip(["prd", "stg"]) {
        let file = format!("::svc/chart/api/values-{env}.yaml::lib-chart.envs");
        assert!(row["workload"].as_str().unwrap().ends_with(&file), "{row}");
        assert_eq!(row["path"], "svc", "{row}");
        assert_eq!(row["level"], "declared", "{row}");
    }
}

#[test]
fn adding_an_environment_file_on_a_warm_index_rederives_the_set() {
    let repo = fixture("helm_env_warm_add");
    let base = values(&[("ORDERS_TOPIC", "orders.created")]);
    write(&repo, &format!("{CHART}/values.yaml"), &base);
    for env in ["stg", "prd"] {
        std::fs::remove_file(repo.join(format!("{CHART}/values-{env}.yaml"))).unwrap();
    }
    let engine = Engine::open(&repo).unwrap();
    let before = export_of(&engine);
    let rows = helm_rows(&before);
    assert_eq!(rows.len(), 1, "{before}");
    assert!(
        rows[0]["workload"]
            .as_str()
            .unwrap()
            .contains("/values.yaml::")
    );
    write(&repo, &format!("{CHART}/values-prd.yaml"), &base);
    let after = export_of(&engine);
    let rows = helm_rows(&after);
    assert_eq!(rows.len(), 1, "{after}");
    assert!(
        rows[0]["workload"]
            .as_str()
            .unwrap()
            .contains("/values-prd.yaml::")
    );
}

#[test]
fn editing_helm_env_rereads_the_values_on_a_warm_index() {
    let repo = fixture("helm_env_warm_edit");
    let engine = Engine::open(&repo).unwrap();
    let before = export_of(&engine);
    assert_eq!(
        consumed(&before, "svc/consumer.go"),
        ["channel::kafka::orders.created"]
    );
    let toml = std::fs::read_to_string(repo.join("meada.toml")).unwrap();
    write(
        &repo,
        "meada.toml",
        &toml.replace("lib-chart.envs", "other.envs"),
    );
    let after = export_of(&engine);
    assert_eq!(consumed(&after, "svc/consumer.go"), ["channel::kafka"]);
}

#[test]
fn without_the_declaration_nothing_changes() {
    let export = export("helm_env_none", |repo| {
        write(
            repo,
            "meada.toml",
            "[serve]\nhosts = [\"orders.example.com\"]\n",
        );
    });
    assert_eq!(consumed(&export, "svc/consumer.go"), ["channel::kafka"]);
    assert!(helm_rows(&export).is_empty(), "{export}");
}

#[test]
fn a_nested_package_gets_no_environment_from_the_chart() {
    let export = export("helm_env_nested", |_| {});
    assert_eq!(
        consumed(&export, "svc/nested/reader.go"),
        ["channel::kafka"]
    );
}

#[test]
fn only_the_owning_package_reaches_the_chart_environments() {
    let repo = fixture("helm_env_affected_scope");
    let chart = |answer: &Value| -> Vec<String> {
        (answer["files"].as_array().unwrap().iter())
            .filter_map(|f| f["path"].as_str())
            .filter(|p| p.starts_with(CHART))
            .map(String::from)
            .collect()
    };
    let nested = of_files(&repo, &["svc/nested/reader.go"]);
    assert_eq!(chart(&nested), Vec::<String>::new(), "{nested}");
    let owned = of_files(&repo, &["svc/consumer.go"]);
    assert!(!chart(&owned).is_empty(), "{owned}");
}

fn other_links(export: &Value) -> Vec<String> {
    (export["links"].as_array().unwrap().iter())
        .filter(|l| !l["contract"].as_str().unwrap().starts_with("channel::"))
        .map(Value::to_string)
        .collect()
}

#[test]
fn http_and_resources_ignore_helm_environments() {
    let change = |repo: &Path| {
        compose(
            repo,
            &[
                ("B_URL", "http://nowhere.example.com"),
                ("BUCKET", "orders-bucket"),
            ],
        );
        let envs = [
            ("ORDERS_TOPIC", "orders.created"),
            ("B_URL", "http://orders.example.com"),
            ("BUCKET", "audit-bucket"),
        ];
        for env in ["stg", "prd"] {
            write(repo, &format!("{CHART}/values-{env}.yaml"), &values(&envs));
        }
    };
    let with = export("helm_env_http_with", change);
    let without = export("helm_env_http_without", |repo| {
        change(repo);
        write(
            repo,
            "meada.toml",
            "[serve]\nhosts = [\"orders.example.com\"]\n",
        );
    });
    assert_eq!(other_links(&with), other_links(&without));
    assert_eq!(level_from(&with, "Created"), "declared");
}

#[test]
fn scalars_are_text_and_nested_values_are_skipped() {
    let repo = fixture("helm_env_scalars");
    let text = "lib-chart:\n  envs:\n    ORDERS_TOPIC: orders.created\n    PARTITIONS: 3\n    \
                ENABLED: true\n    NESTED:\n      A: b\n    LIST:\n      - a\n";
    for env in ["stg", "prd"] {
        write(&repo, &format!("{CHART}/values-{env}.yaml"), text);
    }
    let engine = Engine::open(&repo).unwrap();
    assert_eq!(level_from(&export_of(&engine), "Created"), "declared");
    let node = "example.com/acme/helm-env::svc/chart/api/values-stg.yaml::lib-chart.envs";
    let context = engine.context(node, None).unwrap();
    for shown in ["PARTITIONS", "3", "ENABLED", "true", "ORDERS_TOPIC"] {
        assert!(context.contains(shown), "{shown}: {context}");
    }
    for skipped in ["NESTED", "LIST"] {
        assert!(!context.contains(skipped), "{skipped}: {context}");
    }
}

#[test]
fn a_key_through_a_list_gives_no_environment() {
    let export = export("helm_env_list", |repo| {
        let text = "lib-chart:\n  - envs:\n      ORDERS_TOPIC: orders.created\n";
        for env in ["stg", "prd"] {
            write(repo, &format!("{CHART}/values-{env}.yaml"), text);
        }
    });
    assert_eq!(consumed(&export, "svc/consumer.go"), ["channel::kafka"]);
    assert!(helm_rows(&export).is_empty(), "{export}");
}

#[test]
fn a_value_that_is_no_channel_name_names_nothing() {
    let export = export("helm_env_bad_name", |repo| {
        for env in ["stg", "prd"] {
            let text = values(&[("ORDERS_TOPIC", "\"orders created\"")]);
            write(repo, &format!("{CHART}/values-{env}.yaml"), &text);
        }
    });
    assert_eq!(consumed(&export, "svc/consumer.go"), ["channel::kafka"]);
}

#[test]
fn an_unreadable_values_file_gives_no_node() {
    let export = export("helm_env_unreadable", |repo| {
        for env in ["stg", "prd"] {
            write(
                repo,
                &format!("{CHART}/values-{env}.yaml"),
                "lib-chart: [\n  envs: {\n",
            );
        }
    });
    assert_eq!(consumed(&export, "svc/consumer.go"), ["channel::kafka"]);
    assert!(helm_rows(&export).is_empty(), "{export}");
}
