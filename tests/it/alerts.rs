//! Alert emitters and Alertmanager matchers (#672).

use crate::common::{assert_golden, fixture_repo};
use meada::engine::Engine;

#[test]
fn alert_labels_join_equality_routes_and_helm_values_are_not_emitters() {
    let app = fixture_repo(
        "alerts_equality",
        "alert-app",
        Some("https://example.com/acme/app.git"),
    );
    let repo = fixture_repo(
        "alerts_equality",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 2, "{text}");
    assert_eq!(export["consumers"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(export["links"].as_array().unwrap().len(), 1, "{text}");
    assert_eq!(export["links"][0]["contract"], "alert::support::team-b");
    assert_eq!(export["links"][0]["level"], "exact");
    assert_eq!(export["iac_nodes"].as_array().unwrap().len(), 2, "{text}");
    assert_golden("alerts.equality.export.json", &text);
    let app_export = Engine::open(&app).unwrap().export().unwrap();
    let app: serde_json::Value = serde_json::from_str(&app_export).unwrap();
    assert!(
        app["providers"].as_array().unwrap().is_empty(),
        "{app_export}"
    );
    assert_golden("alerts.app.export.json", &app_export);
}

#[test]
fn monitoring_formats_join_anchored_regex_and_all_equality_conditions() {
    let repo = fixture_repo(
        "alerts_formats",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    std::fs::write(
        repo.join("rules.yaml"),
        r#"groups:
  - name: prometheus
    labels: {region: eu}
    rules:
      - alert: Local
        expr: up == 0
        labels: {support: team-b}
      - alert: RegexLookalike
        expr: up == 0
        labels: {support: team-b-extra}
      - record: recorded_metric
        expr: up
        labels: {support: team-b}
---
apiVersion: monitoring.coreos.com/v1
kind: PrometheusRule
metadata: {name: pods}
spec:
  groups:
    - name: pods
      rules:
        - alert: Pods
          expr: kube_pod_status_phase > 1
          labels: {support: team-b}
---
apiVersion: 1
groups:
  - orgId: 1
    name: grafana
    rules:
      - uid: cpu
        title: Cpu
        condition: A
        data: []
        labels: {support: team-c, severity: normal}
"#,
    )
    .unwrap();
    std::fs::write(repo.join("loki.yaml"), "groups:\n  - name: logs\n    rules:\n      - alert: Logs\n        expr: count_over_time({job=\"api\"}[5m]) > 10\n        labels: {support: team-b}\n").unwrap();
    std::fs::write(
        repo.join("alertmanager.yaml"),
        r#"route:
  receiver: fallback
  routes:
    - receiver: equality
      continue: true
      match: {support: team-b}
    - receiver: regex
      continue: true
      match_re: {support: "team-(b|c)"}
    - receiver: conjunction
      match: {support: team-b, severity: critical}
receivers:
  - name: fallback
  - name: equality
  - name: regex
  - name: conjunction
---
apiVersion: monitoring.coreos.com/v1beta1
kind: AlertmanagerConfig
metadata: {name: grafana}
spec:
  route:
    receiver: grafana-team
    matchers:
      - {name: support, value: team-c, matchType: "="}
    routes:
      - receiver: urgent
        matchers:
          - {name: severity, value: urgent, matchType: "="}
  receivers: [{name: grafana-team}, {name: urgent}]
---
apiVersion: monitoring.coreos.com/v1alpha1
kind: AlertmanagerConfig
metadata: {name: legacy}
spec:
  route:
    receiver: legacy-team
    matchers:
      - {name: support, value: team-b, regex: true}
  receivers: [{name: legacy-team}]
"#,
    )
    .unwrap();
    std::fs::write(
        repo.join("grafana.tf"),
        r#"resource "grafana_rule_group" "infra" {
  name = "infrastructure"
  folder_uid = "ops"
  interval_seconds = 60
  rule {
    name = "Disk"
    condition = "A"
    labels = { support = "team-c", severity = "normal" }
  }
}
"#,
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 14, "{text}");
    assert_eq!(export["consumers"].as_array().unwrap().len(), 8, "{text}");
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 13, "{text}");
    assert_eq!(
        links.iter().filter(|l| l["level"] == "exact").count(),
        5,
        "{text}"
    );
    assert!(
        links.iter().all(|l| !l["provider"]
            .as_str()
            .unwrap()
            .ends_with("groups[0].rules[1]")),
        "{text}"
    );
    assert_golden("alerts.formats.export.json", &text);
    let pattern = engine.context("alert::support::team-(b|c)", None).unwrap();
    assert!(
        pattern.contains("grafana.tf") && pattern.contains("inferred"),
        "{pattern}"
    );
    assert_golden("alerts.pattern.context.txt", &pattern);
    let rule_context = engine
        .context("rules.yaml::[0].groups[0].rules[0]", None)
        .unwrap();
    assert!(
        rule_context.contains("emitter: alert::alertname::Local"),
        "{rule_context}"
    );
    assert_golden("alerts.rule.context.txt", &rule_context);

    let context = engine
        .context("alertmanager.yaml::[0].route.routes[0]", None)
        .unwrap();
    assert!(context.contains("receiver: equality"), "{context}");
    assert_golden("alerts.route.context.txt", &context);
    use meada::engine::{Direction, Start};
    let down = engine
        .trace(
            Start::From("rules.yaml::[0].groups[0].rules[0]"),
            Direction::Down,
            None,
        )
        .unwrap();
    let up = engine
        .trace(
            Start::From("alertmanager.yaml::[0].route.routes[0]"),
            Direction::Up,
            None,
        )
        .unwrap();
    assert!(
        down.contains("alertmanager.yaml") && down.contains("alert::support::team-b"),
        "{down}"
    );
    assert!(
        up.contains("rules.yaml") && up.contains("loki.yaml"),
        "{up}"
    );
    assert_golden("alerts.trace.txt", &format!("{down}\n{up}"));
}

#[test]
fn alert_orphans_require_both_roles_in_scope_and_follow_refresh() {
    let repo = fixture_repo(
        "alerts_orphans",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    std::fs::write(
        repo.join("alertmanager.yaml"),
        "route:\n  receiver: fallback\nreceivers: [{name: fallback}]\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let root_only: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert!(
        root_only["consumers"].as_array().unwrap().is_empty(),
        "{root_only}"
    );
    assert!(
        root_only["links"].as_array().unwrap().is_empty(),
        "{root_only}"
    );
    let status = |engine: &Engine| {
        engine
            .status_report()
            .unwrap()
            .1
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n")
    };
    let search = engine
        .search("alert::support::team-b", Some("alert"), None)
        .unwrap();
    assert!(search.contains("no matcher in the workspace"), "{search}");
    assert!(!status(&engine).contains("orphan alert::"));
    std::fs::write(
        repo.join("alertmanager.yaml"),
        "route:\n  receiver: team-a\n  match: {support: team-a}\nreceivers: [{name: team-a}]\n",
    )
    .unwrap();
    let report = status(&engine);
    assert!(
        report.contains("orphan alert::support::team-b: no matcher"),
        "{report}"
    );
    assert!(
        report.contains("orphan alert::support::team-a: no emitter"),
        "{report}"
    );
    let original = std::fs::read_to_string(repo.join("rules.yaml")).unwrap();
    std::fs::write(
        repo.join("rules.yaml"),
        original.replace("team-b", "team-a"),
    )
    .unwrap();
    let matched: serde_json::Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    assert_eq!(matched["links"].as_array().unwrap().len(), 1, "{matched}");
    let report = status(&engine);
    let orphans = report
        .lines()
        .filter(|l| l.starts_with("orphan alert::"))
        .collect::<Vec<_>>();
    assert_eq!(
        orphans,
        ["orphan alert::alertname::ApiDown: no matcher; rules.yaml::groups[0].rules[0]"],
        "{report}"
    );
    std::fs::remove_file(repo.join("rules.yaml")).unwrap();
    let context = engine.context("alert::support::team-a", None).unwrap();
    assert!(context.contains("no emitter in the workspace"), "{context}");
    assert!(!status(&engine).contains("orphan alert::"));
}

#[test]
fn unknown_labels_do_not_choose_a_later_route_and_known_siblings_stop_at_the_first_match() {
    let repo = fixture_repo(
        "alerts_order",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    std::fs::write(
        repo.join("rules.yaml"),
        r#"groups:
  - name: routing
    rules:
      - alert: UnknownRegion
        expr: up == 0
        labels: {support: team-b, region: "{{ $labels.region }}"}
      - alert: America
        expr: up == 0
        labels: {support: team-b, region: us}
      - alert: Europe
        expr: up == 0
        labels: {support: team-b, region: eu}
"#,
    )
    .unwrap();
    std::fs::write(
        repo.join("alertmanager.yaml"),
        r#"route:
  receiver: fallback
  routes:
    - receiver: europe
      matchers: ['region="eu"']
    - receiver: support
      matchers: ['support="team-b"']
receivers: [{name: fallback}, {name: europe}, {name: support}]
"#,
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    assert_eq!(
        links[0]["provider"],
        "example.com/acme/monitoring::rules.yaml::groups[0].rules[2]"
    );
    assert_eq!(
        links[1]["provider"],
        "example.com/acme/monitoring::rules.yaml::groups[0].rules[1]"
    );
    assert!(links.iter().all(|l| l["level"] == "exact"), "{text}");
}

#[test]
fn grafana_exports_and_opentofu_labels_reach_negative_matchers_without_shadowed_definitions() {
    let repo = fixture_repo(
        "alerts_grafana_exports",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    std::fs::remove_file(repo.join("rules.yaml")).unwrap();
    std::fs::write(repo.join("standalone.json"), r#"{"uid":"standalone","title":"Standalone","condition":"A","data":[],"labels":{"support":"team-d"}}"#).unwrap();
    std::fs::write(
        repo.join("almost.json"),
        r#"{"title":"NotARule","labels":{"support":"team-d"}}"#,
    )
    .unwrap();
    let hcl = "resource \"grafana_rule_group\" \"ops\" {\n  rule {\n    name = \"Disk\"\n    labels = { support = \"team-d\" }\n  }\n}\n";
    std::fs::write(repo.join("group.tf"), hcl.replace("team-d", "shadowed")).unwrap();
    std::fs::write(repo.join("group.tofu"), hcl).unwrap();
    let json = r#"{"resource":{"grafana_rule_group":{"ops":{"rule":[{"name":"Memory","labels":{"support":"team-d"}}]}}}}"#;
    std::fs::write(
        repo.join("other.tf.json"),
        json.replace("team-d", "shadowed"),
    )
    .unwrap();
    let pretty =
        serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(json).unwrap())
            .unwrap();
    std::fs::write(repo.join("other.tofu.json"), pretty).unwrap();
    std::fs::write(
        repo.join("alertmanager.yaml"),
        r#"route:
  receiver: fallback
  routes:
    - receiver: not-e
      continue: true
      matchers: ['support!="team-e"']
    - receiver: not-ef
      matchers: ['support!~"team-(e|f)"']
receivers: [{name: fallback}, {name: not-e}, {name: not-ef}]
"#,
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    let providers = export["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 3, "{text}");
    assert!(
        providers
            .iter()
            .all(|p| p["contract"] == "alert::support::team-d"),
        "{text}"
    );
    assert_eq!(export["consumers"].as_array().unwrap().len(), 2, "{text}");
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 6, "{text}");
    assert_eq!(
        links.iter().filter(|l| l["level"] == "exact").count(),
        3,
        "{text}"
    );
    let json_node = export["iac_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "other.tofu.json")
        .unwrap();
    assert!(json_node["line"].as_u64().unwrap() > 1, "{text}");
    assert_golden("alerts.grafana.export.json", &text);
}

#[test]
fn mixed_route_conditions_keep_each_matchers_level_and_explain_the_whole_conjunction() {
    let repo = fixture_repo(
        "alerts_mixed",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    std::fs::write(
        repo.join("rules.yaml"),
        "groups:\n  - name: mixed\n    rules:\n      - alert: Match\n        expr: up == 0\n        labels: {support: team-b, severity: critical}\n      - alert: WrongSeverity\n        expr: up == 0\n        labels: {support: team-b, severity: warning}\n",
    ).unwrap();
    std::fs::write(
        repo.join("alertmanager.yaml"),
        "route:\n  receiver: fallback\n  routes:\n    - receiver: mixed\n      matchers: ['support=\"team-b\"', 'severity=~\"crit.*\"']\nreceivers: [{name: fallback}, {name: mixed}]\n",
    ).unwrap();
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    let links = export["links"].as_array().unwrap();
    let actual = links
        .iter()
        .map(|l| {
            (
                l["contract"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
                l["provider"].as_str().unwrap(),
                l["consumer"].as_str().unwrap(),
                l["reason"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (
                "alert::severity::crit.*",
                "inferred",
                "example.com/acme/monitoring::rules.yaml::groups[0].rules[0]",
                "example.com/acme/monitoring::alertmanager.yaml::route.routes[0]",
                "alert::severity::crit.*; matchers \"support\"=\"team-b\", \"severity\"=~\"crit.*\"; receiver: mixed"
            ),
            (
                "alert::support::team-b",
                "exact",
                "example.com/acme/monitoring::rules.yaml::groups[0].rules[0]",
                "example.com/acme/monitoring::alertmanager.yaml::route.routes[0]",
                "alert::support::team-b; matchers \"support\"=\"team-b\", \"severity\"=~\"crit.*\"; receiver: mixed"
            ),
        ],
        "{text}"
    );
}

#[test]
fn uninterpretable_route_matchers_do_not_guess_a_destination() {
    let repo = fixture_repo(
        "alerts_invalid",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    for matcher in [
        "[",
        "(?x:team - b)",
        "(?u:team-b)",
        "[t[e]]eam-b",
        r"[\Qt\E]eam-b",
        "(?P<a.b>team-b)",
        "(?P<é>team-b)",
        "(?<a.b>team-b)",
        "(?<é>team-b)",
        r"(?P<\Qname\E>team-b)",
        r"(?<\Qname\E>team-b)",
        "team-b{01}",
        "team-b{ 1}",
        "{{ .Values.team }}",
    ] {
        let config = serde_json::json!({
            "route": {"receiver": "invalid", "match_re": {"support": matcher}},
            "receivers": [{"name": "invalid"}]
        });
        std::fs::write(repo.join("alertmanager.yaml"), config.to_string()).unwrap();
        let text = engine.export().unwrap();
        let export: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(
            export["links"].as_array().unwrap().is_empty(),
            "{matcher}: {text}"
        );
    }
}

#[test]
fn alert_places_do_not_change_search_locations_of_other_kinds() {
    let repo = fixture_repo(
        "alerts_foreign_places",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    std::fs::write(
        repo.join("main.tf"),
        "resource \"aws_sqs_queue\" \"q\" { name = \"q\" }\n",
    )
    .unwrap();
    std::fs::write(repo.join("meada.toml"), "[[resource]]\nkind = \"routing\"\nid = \"teams\"\nrole = \"write\"\nvia = [\"main.tf::aws_sqs_queue.q\"]\n").unwrap();
    let found = Engine::open(&repo)
        .unwrap()
        .search("resource::routing::teams", Some("resource"), None)
        .unwrap();
    assert_eq!(
        found.trim(),
        "example.com/acme/monitoring aws_sqs_queue.q resource::routing::teams (no provisioner in the workspace)"
    );
}

#[test]
fn unrelated_repositories_do_not_repeat_alert_routing_for_status_and_context() {
    unrelated_alert_repositories(60);
}

#[test]
fn many_unrelated_repositories_keep_alert_queries_bounded() {
    unrelated_alert_repositories(250);
}

fn unrelated_alert_repositories(repository_count: usize) {
    let repo = fixture_repo(
        &format!("alerts_many_repositories_{repository_count}"),
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    let rules = (0..1000).map(|i| serde_json::json!({"alert":format!("R{i}"), "expr":"up", "labels":{"support":"other"}})).collect::<Vec<_>>();
    let routes = (0..600).map(|_| serde_json::json!({"receiver":"team", "match":{"support":"team-b"}, "continue":true})).collect::<Vec<_>>();
    std::fs::write(
        repo.join("rules.yaml"),
        serde_json::json!({"groups":[{"name":"wide", "rules":rules}]}).to_string(),
    )
    .unwrap();
    std::fs::write(
        repo.join("alertmanager.yaml"),
        serde_json::json!({"route":{"receiver":"fallback", "routes":routes}, "receivers":[]})
            .to_string(),
    )
    .unwrap();
    let mut manifest = String::new();
    for i in 0..repository_count - 1 {
        let empty = repo.parent().unwrap().join(format!("empty-{i}"));
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        crate::common::git(&empty, &["init", "-q"]);
        crate::common::git(
            &empty,
            &[
                "remote",
                "add",
                "origin",
                &format!("https://example.com/acme/empty-{i}.git"),
            ],
        );
        crate::common::git(&empty, &["commit", "--allow-empty", "-qm", "empty"]);
        manifest.push_str(&format!(
            "[[peer]]\nrepo = \"example.com/acme/empty-{i}\"\n"
        ));
    }
    std::fs::write(repo.join("meada.toml"), manifest).unwrap();
    let engine = Engine::open(&repo).unwrap();
    engine.export().unwrap();
    let started = std::time::Instant::now();
    let (_, status) = engine.status_report().unwrap();
    let mut expected_status = vec![Vec::<String>::new(); repository_count];
    expected_status[0] = vec![
        "alert routing skipped: evaluation limit".into(),
        "config files: 2 read, 0 skipped (lockfiles: 0, unreadable: 0); values: 4802, redacted: 0"
            .into(),
    ];
    assert_eq!(status, expected_status);
    let context = engine
        .context("alert::support::team-b", Some(1_000_000))
        .unwrap();
    let mut expected = (0..600)
        .map(|i| {
            format!("  example.com/acme/monitoring matcher alertmanager.yaml:1 route.routes[{i}]")
        })
        .collect::<Vec<_>>();
    expected.sort();
    expected.insert(0, "alert::support::team-b".into());
    eprintln!(
        "{repository_count}-repository alert status/context: {:?}",
        started.elapsed()
    );
    assert_eq!(
        context.lines().collect::<Vec<_>>(),
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
}

#[test]
fn unwritten_series_labels_do_not_choose_a_sibling_or_satisfy_a_negative_condition() {
    let repo = fixture_repo(
        "alerts_series",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    for routes in [
        serde_json::json!([
            {"receiver":"europe", "match":{"region":"eu"}},
            {"receiver":"support", "match":{"support":"team-b"}}
        ]),
        serde_json::json!([
            {"receiver":"support", "matchers":["support=\"team-b\"", "region!=\"eu\""]}
        ]),
    ] {
        let config = serde_json::json!({
            "route":{"receiver":"fallback", "routes":routes},
            "receivers":[{"name":"fallback"}, {"name":"europe"}, {"name":"support"}]
        });
        std::fs::write(repo.join("alertmanager.yaml"), config.to_string()).unwrap();
        let text = engine.export().unwrap();
        let export: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(export["links"].as_array().unwrap().is_empty(), "{text}");
    }
}

#[test]
fn a_computed_grafana_label_key_can_override_a_written_label() {
    let repo = fixture_repo(
        "alerts_unknown_key",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    std::fs::remove_file(repo.join("rules.yaml")).unwrap();
    std::fs::write(repo.join("grafana.tf"), "resource \"grafana_rule_group\" \"ops\" {\n rule {\n  name = \"Disk\"\n  labels = { support = \"team-b\", (var.label) = \"team-c\" }\n }\n}\n").unwrap();
    std::fs::write(repo.join("grafana.tf.json"), r#"{"resource":{"grafana_rule_group":{"ops":{"rule":[{"name":"Other","labels":{"support":"team-b","${var.label}":"team-c"}}]}}}}"#).unwrap();
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(export["providers"].as_array().unwrap().is_empty(), "{text}");
    assert!(export["links"].as_array().unwrap().is_empty(), "{text}");
}

#[test]
fn many_continuing_matchers_evaluate_each_rule_once_per_tree() {
    let repo = fixture_repo(
        "alerts_many_routes",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    let rules = (0..10)
        .map(|i| {
            serde_json::json!({
                "alert":format!("Rule{i}"), "expr":"up == 0", "labels":{"support":"team-b"}
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        repo.join("rules.yaml"),
        serde_json::json!({"groups":[{"name":"many", "rules":rules}]}).to_string(),
    )
    .unwrap();
    let routes = (0..400)
        .map(|_| {
            serde_json::json!({
                "receiver":"support", "match_re":{"support":"team-b"}, "continue":true
            })
        })
        .collect::<Vec<_>>();
    std::fs::write(
        repo.join("alertmanager.yaml"),
        serde_json::json!({
            "route":{"receiver":"support", "routes":routes}, "receivers":[{"name":"support"}]
        })
        .to_string(),
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let started = std::time::Instant::now();
    let text = engine.export().unwrap();
    let elapsed = started.elapsed();
    eprintln!("400 routes and 10 rules: {elapsed:?}");
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 20, "{text}");
    assert_eq!(export["consumers"].as_array().unwrap().len(), 400, "{text}");
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 4000, "{text}");
    assert!(links.iter().all(|l| l["level"] == "inferred"), "{text}");
    assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
}

#[test]
fn prometheus_alertname_is_an_emitter_even_without_labels_and_overrides_the_label_map() {
    let repo = fixture_repo(
        "alerts_alertname",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    std::fs::write(repo.join("rules.yaml"), "groups:\n  - name: names\n    rules:\n      - alert: HighCPU\n        expr: up == 0\n      - alert: ApiDown\n        expr: up == 0\n        labels: {alertname: WrongName}\n").unwrap();
    std::fs::write(repo.join("alertmanager.yaml"), "route:\n  receiver: fallback\n  routes:\n    - receiver: cpu\n      match: {alertname: HighCPU}\n    - receiver: api\n      match: {alertname: ApiDown}\nreceivers: [{name: fallback}, {name: cpu}, {name: api}]\n").unwrap();
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        export["providers"],
        serde_json::json!([
            {"contract":"alert::alertname::ApiDown", "site":"example.com/acme/monitoring::rules.yaml::groups[0].rules[1]"},
            {"contract":"alert::alertname::HighCPU", "site":"example.com/acme/monitoring::rules.yaml::groups[0].rules[0]"}
        ]),
        "{text}"
    );
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 2, "{text}");
    assert!(links.iter().all(|l| l["level"] == "exact"), "{text}");
    assert_golden("alerts.alertname.export.json", &text);
}

#[test]
fn distinct_route_regexes_share_one_automaton_budget() {
    let repo = fixture_repo(
        "alerts_regex_budget",
        "alert-monitoring",
        Some("https://example.com/acme/monitoring.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    for (count, expected) in [(1, 1), (8, 0)] {
        let routes = (0..count)
            .map(|i| {
                serde_json::json!({
                    "receiver":"support", "continue":true,
                    "match_re":{"support":format!("team-b|{}{i}", "x{1000}".repeat(8))}
                })
            })
            .collect::<Vec<_>>();
        std::fs::write(
            repo.join("alertmanager.yaml"),
            serde_json::json!({
                "route":{"receiver":"support", "routes":routes}, "receivers":[{"name":"support"}]
            })
            .to_string(),
        )
        .unwrap();
        let text = engine.export().unwrap();
        let export: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            export["consumers"].as_array().unwrap().len(),
            count,
            "{text}"
        );
        assert_eq!(
            export["links"].as_array().unwrap().len(),
            expected,
            "{text}"
        );
    }
}
