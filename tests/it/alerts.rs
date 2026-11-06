//! Alert emitters, receiver forwarding and native incident matchers (#672, #673).

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

#[test]
fn public_638_monitor_tags_reach_the_original_service_matcher() {
    use meada::engine::{Direction, Start};
    let app = fixture_repo(
        "alerts_638",
        "alert-638-app",
        Some("https://example.com/acme/app.git"),
    );
    fixture_repo(
        "alerts_638",
        "alert-638-routing",
        Some("https://example.com/acme/routing.git"),
    );
    let monitors = fixture_repo(
        "alerts_638",
        "alert-638-monitors",
        Some("https://example.com/acme/monitors.git"),
    );
    let engine = Engine::open(&monitors).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 2, "{text}");
    assert!(export["consumers"].as_array().unwrap().is_empty(), "{text}");
    let routing_text = Engine::open(&monitors.parent().unwrap().join("alert-638-routing"))
        .unwrap()
        .export()
        .unwrap();
    let routing: serde_json::Value = serde_json::from_str(&routing_text).unwrap();
    assert_eq!(
        routing["consumers"].as_array().unwrap().len(),
        1,
        "{routing_text}"
    );
    let links = export["links"].as_array().unwrap();
    assert_eq!(links.len(), 1, "{text}");
    assert_eq!(links[0]["contract"], "alert::service::app");
    assert_eq!(links[0]["level"], "inferred");
    assert_eq!(
        links[0]["provider"],
        "example.com/acme/monitors::main.tf::datadog_monitor.app"
    );
    assert_eq!(
        links[0]["consumer"],
        "example.com/acme/routing::teams/platform/routing-rules.tf::opsgenie_team_routing_rule.app"
    );
    assert_golden("alerts.638.export.json", &text);
    let support = engine.context("alert::support::team-b", None).unwrap();
    assert_golden("alerts.638.support.context.txt", &support);
    let trace = engine
        .trace(Start::From("alert::support::team-b"), Direction::Down, None)
        .unwrap();
    assert!(
        trace.contains("alert::service::app") && trace.contains("opsgenie_team_routing_rule.app"),
        "{trace}"
    );
    assert_golden("alerts.638.trace.txt", &trace);
    let app_text = Engine::open(&app).unwrap().export().unwrap();
    let app_export: serde_json::Value = serde_json::from_str(&app_text).unwrap();
    assert!(
        app_export["providers"].as_array().unwrap().is_empty(),
        "{app_text}"
    );
}

#[test]
fn selected_receivers_transform_grouped_labels_into_explicit_incident_fields() {
    let repo = fixture_repo(
        "alerts_forwarding",
        "alert-forwarding",
        Some("https://example.com/acme/forwarding.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let text = engine.export().unwrap();
    let export: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(export["providers"].as_array().unwrap().len(), 6, "{text}");
    assert_eq!(export["consumers"].as_array().unwrap().len(), 3, "{text}");
    let actual = export["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["contract"].as_str().unwrap(),
                l["level"].as_str().unwrap(),
                l["provider"].as_str().unwrap(),
                l["consumer"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            (
                "alert::support::team-b",
                "exact",
                "example.com/acme/forwarding::rules.yaml::groups[0].rules[0]",
                "example.com/acme/forwarding::alertmanager.yaml::route.routes[0]"
            ),
            (
                "alert::target::team-c",
                "exact",
                "example.com/acme/forwarding::rules.yaml::groups[0].rules[0]",
                "example.com/acme/forwarding::main.tf::opsgenie_team_routing_rule.transformed"
            ),
            (
                "alert::support::team-b",
                "exact",
                "example.com/acme/forwarding::rules.yaml::groups[0].rules[0]",
                "example.com/acme/forwarding::main.tf::pagerduty_event_orchestration_service.checkout.set[0].rule[0]"
            ),
        ],
        "{text}"
    );
    assert_golden("alerts.forwarding.export.json", &text);
    let transformed = engine.context("alert::target::team-c", None).unwrap();
    assert!(
        transformed.contains("emitter rules.yaml:4 groups[0].rules[0]"),
        "{transformed}"
    );
    assert_golden("alerts.forwarding.target.context.txt", &transformed);
    let context = engine
        .context(
            "pagerduty_event_orchestration_service.checkout.set[0].rule[0]",
            None,
        )
        .unwrap();
    assert!(context.contains("receiver: checkout-id"), "{context}");
    assert_golden("alerts.forwarding.context.txt", &context);
    use meada::engine::{Direction, Start};
    let trace = engine
        .trace(
            Start::From("rules.yaml::groups[0].rules[0]"),
            Direction::Down,
            None,
        )
        .unwrap();
    assert!(
        trace.contains("forwarding:")
            && trace.contains("opsgenie_team_routing_rule.transformed")
            && trace.contains("pagerduty_event_orchestration_service.checkout"),
        "{trace}"
    );
    assert_golden("alerts.forwarding.trace.txt", &trace);
}

#[test]
fn incident_receiver_filters_templates_grouping_and_team_scope_never_invent_forwarding() {
    let repo = fixture_repo(
        "alerts_receiver_controls",
        "alert-forwarding",
        Some("https://example.com/acme/forwarding.git"),
    );
    let engine = Engine::open(&repo).unwrap();
    let original = std::fs::read_to_string(repo.join("alertmanager.yaml")).unwrap();
    let rows = |text: String| {
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["contract"].as_str().unwrap().to_string(),
                    l["consumer"].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>()
    };
    let am = (
        "alert::support::team-b".to_string(),
        "example.com/acme/forwarding::alertmanager.yaml::route.routes[0]".to_string(),
    );
    let og = (
        "alert::target::team-c".to_string(),
        "example.com/acme/forwarding::main.tf::opsgenie_team_routing_rule.transformed".to_string(),
    );
    let pd = ("alert::support::team-b".to_string(), "example.com/acme/forwarding::main.tf::pagerduty_event_orchestration_service.checkout.set[0].rule[0]".to_string());
    assert_eq!(
        rows(engine.export().unwrap()),
        [am.clone(), og.clone(), pd.clone()]
    );
    for (source, expected) in [
        (
            original.replace("match: {support: team-b}", "match: {support: team-z}"),
            Vec::new(),
        ),
        (
            original.replace(
                "    opsgenie_configs:\n      - tags: 'target:{{ .CommonLabels.owner }}'\n",
                "",
            ),
            vec![am.clone(), pd.clone()],
        ),
        (
            original.replace(".CommonLabels.owner", ".Labels.owner"),
            vec![am.clone(), pd.clone()],
        ),
        (
            original.replace(".CommonLabels.owner", ".CommonLabels.absent"),
            vec![am.clone(), pd.clone()],
        ),
        (
            original.replace("[support, owner]", "[support]"),
            vec![am.clone(), pd.clone()],
        ),
        (original.replace("[support, owner]", "[]"), vec![am.clone()]),
        (
            original.replace(
                "      - tags:",
                "      - responders: [{type: team, id: another-team-id}]\n        tags:",
            ),
            vec![am.clone(), pd.clone()],
        ),
        (
            original.replace("routing_key:", "service_key:"),
            vec![am.clone(), og.clone()],
        ),
    ] {
        std::fs::write(repo.join("alertmanager.yaml"), &source).unwrap();
        assert_eq!(rows(engine.export().unwrap()), expected, "{source}");
    }
    let scoped = original.replace(
        "      - tags:",
        "      - responders: [{type: team, id: team-c-id}]\n        tags:",
    );
    std::fs::write(repo.join("alertmanager.yaml"), scoped).unwrap();
    assert_eq!(
        rows(engine.export().unwrap()),
        [am.clone(), og.clone(), pd.clone()]
    );
    std::fs::write(repo.join(".meadaignore"), "main.tf\n").unwrap();
    assert_eq!(rows(engine.export().unwrap()), [am.clone()]);
    std::fs::remove_file(repo.join(".meadaignore")).unwrap();
    std::fs::write(repo.join("alertmanager.yaml"), &original).unwrap();
    assert_eq!(
        rows(engine.export().unwrap()),
        [am.clone(), og.clone(), pd.clone()]
    );
    let native = std::fs::read_to_string(repo.join("main.tf")).unwrap();
    for scope in [
        "var.team_id",
        "var.team.id",
        "{ reference = \"team-c-id\" }",
    ] {
        std::fs::write(repo.join("main.tf"), native.replace("\"team-c-id\"", scope)).unwrap();
        assert_eq!(rows(engine.export().unwrap()), [am.clone(), pd.clone()]);
    }
    for scope in ["var.service.id", "{ reference = \"checkout-id\" }"] {
        std::fs::write(
            repo.join("main.tf"),
            native.replace("\"checkout-id\"", scope),
        )
        .unwrap();
        assert_eq!(rows(engine.export().unwrap()), [am.clone(), og.clone()]);
    }
    let references = native
        .replace("\"team-c-id\"", "opsgenie_team.delivery.id")
        .replace("\"checkout-id\"", "pagerduty_service.checkout.id");
    std::fs::write(repo.join("main.tf"), references).unwrap();
    assert_eq!(
        rows(engine.export().unwrap()),
        [am.clone(), og.clone(), pd.clone()]
    );
    let conjunction = native.replace("    conditions {", "    conditions {\n      field = \"tags\"\n      operation = \"equals\"\n      expected_value = \"urgent\"\n    }\n    conditions {");
    std::fs::write(repo.join("main.tf"), conjunction).unwrap();
    std::fs::write(
        repo.join("alertmanager.yaml"),
        original.replace("tags: 'target:", "tags: 'urgent,target:"),
    )
    .unwrap();
    let plain = ("alert::tags::urgent".to_string(), og.1.clone());
    assert_eq!(
        rows(engine.export().unwrap()),
        [am.clone(), plain, og.clone(), pd.clone()]
    );
    std::fs::write(repo.join("alertmanager.yaml"), &original).unwrap();
    assert_eq!(rows(engine.export().unwrap()), [am.clone(), pd.clone()]);
    std::fs::write(repo.join("main.tf"), &native).unwrap();
    assert_eq!(rows(engine.export().unwrap()), [am, og, pd]);
}

#[test]
fn native_opsgenie_conditions_test_complete_tags_at_their_written_levels() {
    let repo = fixture_repo(
        "alerts_ops_conditions",
        "alert-forwarding",
        Some("https://example.com/acme/ops.git"),
    );
    for path in ["rules.yaml", "alertmanager.yaml", "main.tf"] {
        std::fs::remove_file(repo.join(path)).unwrap();
    }
    std::fs::write(
        repo.join("monitors.tf"),
        r#"resource "datadog_monitor" "literal" {
  name = "Literal"
  tags = ["support:team-b", "region:eu", "urgent"]
}
resource "datadog_monitor" "lookalike" {
  name = "Lookalike"
  tags = ["support:otherteam-b"]
}
resource "datadog_monitor" "unknown" {
  name = "Unknown"
  tags = ["support:team-b", var.dynamic_tag]
}
"#,
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    for (operation, expected, contract, level, names) in [
        (
            "equals",
            "urgent",
            "alert::tags::urgent",
            "exact",
            vec!["literal"],
        ),
        (
            "equals",
            "support:team-b",
            "alert::support::team-b",
            "exact",
            vec!["literal", "unknown"],
        ),
        (
            "matches",
            "support:team-.*",
            "alert::support::team-.*",
            "inferred",
            vec!["literal", "unknown"],
        ),
        (
            "matches",
            ".upport:team-b",
            "alert::.upport::team-b",
            "inferred",
            vec!["literal", "unknown"],
        ),
        (
            "contains",
            "port:team-b",
            "alert::port::team-b",
            "inferred",
            vec!["literal", "unknown"],
        ),
        (
            "starts-with",
            "support:team-",
            "alert::support::team-",
            "inferred",
            vec!["literal", "unknown"],
        ),
        (
            "contains",
            "support:team-b",
            "alert::support::team-b",
            "inferred",
            vec!["literal", "unknown"],
        ),
        (
            "contains",
            "team-b",
            "alert::tags::team-b",
            "inferred",
            vec!["literal", "lookalike", "unknown"],
        ),
        (
            "ends-with",
            "team-b",
            "alert::tags::team-b",
            "inferred",
            vec!["literal", "lookalike", "unknown"],
        ),
    ] {
        let source = format!(
            "resource \"opsgenie_team_routing_rule\" \"test\" {{\n team_id = \"team\"\n criteria {{\n type = \"match-all-conditions\"\n conditions {{\n field = \"tags\"\n operation = \"{operation}\"\n expected_value = \"{expected}\"\n }}\n }}\n notify {{ type = \"none\" }}\n}}\n"
        );
        std::fs::write(repo.join("route.tf"), source).unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let actual = v["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["contract"].as_str().unwrap(),
                    l["level"].as_str().unwrap(),
                    l["provider"].as_str().unwrap().rsplit('.').next().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let expected = names
            .into_iter()
            .map(|name| (contract, level, name))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{operation}: {text}");
        assert!(
            v["links"].as_array().unwrap().iter().all(|l| l["consumer"]
                == "example.com/acme/ops::route.tf::opsgenie_team_routing_rule.test"),
            "{text}"
        );
    }
    let condition = |field: &str, operation: &str, value: &str| {
        format!(
            "conditions {{\n field = \"{field}\"\n operation = \"{operation}\"\n expected_value = {}\n}}\n",
            serde_json::to_string(value).unwrap()
        )
    };
    for extra in [
        condition("tags", "matches", "["),
        condition("priority", "equals", "P1"),
        condition("tags", "contains-key", "support:team-b"),
    ] {
        let source = format!(
            "resource \"opsgenie_team_routing_rule\" \"test\" {{\n team_id = \"team\"\n criteria {{\n type = \"match-all-conditions\"\n {}{extra}\n }}\n notify {{ type = \"none\" }}\n}}\n",
            condition("tags", "equals", "support:team-b")
        );
        std::fs::write(repo.join("route.tf"), source).unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["links"].as_array().unwrap().is_empty(), "{text}");
    }
}

#[test]
fn pagerduty_conditions_keep_native_field_paths_boolean_groups_and_first_match_order() {
    let repo = fixture_repo(
        "alerts_pd_conditions",
        "alert-forwarding",
        Some("https://example.com/acme/pd.git"),
    );
    let receiver = std::fs::read_to_string(repo.join("alertmanager.yaml")).unwrap();
    std::fs::write(
        repo.join("alertmanager.yaml"),
        receiver.replace(
            "          support:",
            "          owner: '{{ .CommonLabels.owner }}'\n          support:",
        ),
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    for (expression, expected) in [
        (
            "event.custom_details.support matches 'team-b'",
            vec![("alert::support::team-b", "exact")],
        ),
        (
            "event.custom_details.owner matches regex 'team-.*'",
            vec![("alert::owner::team-.*", "inferred")],
        ),
        (
            "event.custom_details.owner matches part 'am-'",
            vec![("alert::owner::am-", "inferred")],
        ),
        (
            "event.custom_details.support matches 'team-b' and (event.custom_details.owner matches regex 'team-.*' or event.custom_details.absent matches 'x')",
            vec![
                ("alert::owner::team-.*", "inferred"),
                ("alert::support::team-b", "exact"),
            ],
        ),
        (
            "event.custom_details.support matches 'team-a' or event.custom_details.owner matches 'team-c'",
            vec![("alert::owner::team-c", "exact")],
        ),
        (
            "not event.custom_details.owner matches 'team-z'",
            vec![("alert::owner::team-z", "exact")],
        ),
        (
            "not (event.custom_details.support matches 'team-a' or event.custom_details.owner matches 'team-z')",
            vec![
                ("alert::owner::team-z", "exact"),
                ("alert::support::team-a", "exact"),
            ],
        ),
        ("not event.custom_details.absent matches 'team-b'", vec![]),
        ("event.severity matches 'team-b'", vec![]),
        ("event.custom_details.tags matches 'support:team-b'", vec![]),
        (
            "event.custom_details.owner matches regex '(?P<a.b>team-c)'",
            vec![],
        ),
        (
            "event.custom_details.support matches 'team-b' and unknown()",
            vec![],
        ),
    ] {
        let source = format!(
            "resource \"pagerduty_event_orchestration_service\" \"test\" {{\n service = \"pd-test\"\n set {{\n id = \"start\"\n rule {{\n condition {{ expression = {} }}\n actions {{}}\n }}\n }}\n catch_all {{\n actions {{}}\n }}\n}}\n",
            serde_json::to_string(expression).unwrap()
        );
        std::fs::write(repo.join("main.tf"), source).unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let actual = v["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["contract"].as_str().unwrap(),
                    l["level"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let mut all = vec![("alert::support::team-b", "exact")];
        all.extend(expected);
        assert_eq!(actual, all, "{expression}: {text}");
    }
    let source = r#"resource "pagerduty_event_orchestration_service" "test" {
  service = "pd-test"
  set {
    id = "start"
    rule {
      condition { expression = "event.custom_details.support matches 'team-b'" }
      actions { route_to = "next" }
    }
  }
  set {
    id = "next"
    rule {
      condition { expression = "event.custom_details.owner matches 'team-c'" }
      actions {}
    }
    rule {
      condition { expression = "event.custom_details.support matches 'team-b'" }
      actions {}
    }
  }
  catch_all {
    actions {}
  }
}

"#;
    std::fs::write(repo.join("main.tf"), source).unwrap();
    let text = engine.export().unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["links"].as_array().unwrap().len(), 3, "{text}");
    assert!(
        v["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| !l["consumer"].as_str().unwrap().ends_with("set[1].rule[1]")),
        "{text}"
    );
    let unknown_first = source.replace(
        "event.custom_details.owner matches 'team-c'",
        "event.custom_details.absent matches 'team-c'",
    );
    std::fs::write(repo.join("main.tf"), unknown_first).unwrap();
    let text = engine.export().unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["links"].as_array().unwrap().len(), 2, "{text}");
    assert!(
        v["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| !l["consumer"].as_str().unwrap().contains("set[1]")),
        "{text}"
    );
}

#[test]
fn opsgenie_native_order_and_three_valued_conjunctions_cross_files_in_one_scope() {
    let repo = fixture_repo(
        "alerts_og_order",
        "alert-forwarding",
        Some("https://example.com/acme/order.git"),
    );
    for path in ["rules.yaml", "alertmanager.yaml", "main.tf"] {
        std::fs::remove_file(repo.join(path)).unwrap();
    }
    std::fs::write(
        repo.join("monitors.tf"),
        r#"resource "datadog_monitor" "known" {
 name = "Known"
 tags = ["support:team-b", "region:eu"]
}
resource "datadog_monitor" "unknown" {
 name = "Unknown"
 tags = ["support:team-b", var.tag]
}
"#,
    )
    .unwrap();
    let condition = |field: &str, op: &str, value: &str| {
        format!(
            "conditions {{\n field = {field:?}\n operation = {op:?}\n expected_value = {value:?}\n}}\n"
        )
    };
    let route = |name: &str, order: &str, mode: &str, conditions: String| {
        format!(
            "resource \"opsgenie_team_routing_rule\" {name:?} {{\n team_id = \"team\"\n {order}\n criteria {{\n type = {mode:?}\n {conditions}\n }}\n notify {{ type = \"none\" }}\n}}\n"
        )
    };
    let second = route(
        "second",
        "order = 1",
        "match-all-conditions",
        condition("tags", "equals", "support:team-b"),
    );
    std::fs::write(repo.join("b.tf"), &second).unwrap();
    let engine = Engine::open(&repo).unwrap();
    for (mode, first, expected) in [
        (
            "match-all-conditions",
            condition("tags", "equals", "support:team-a"),
            vec![("known", "second", "alert::support::team-b", "exact")],
        ),
        (
            "match-all-conditions",
            condition("tags", "equals", "support:team-a") + &condition("priority", "equals", "P1"),
            vec![("known", "second", "alert::support::team-b", "exact")],
        ),
        (
            "match-any-condition",
            condition("tags", "equals", "support:team-b") + &condition("priority", "equals", "P1"),
            vec![
                ("known", "first", "alert::support::team-b", "exact"),
                ("unknown", "first", "alert::support::team-b", "exact"),
            ],
        ),
        (
            "match-all-conditions",
            condition("tags", "equals", "support:team-b")
                + &condition("tags", "matches", "region:e.*"),
            vec![
                ("known", "first", "alert::region::e.*", "inferred"),
                ("known", "first", "alert::support::team-b", "exact"),
            ],
        ),
    ] {
        std::fs::write(repo.join("a.tf"), route("first", "order = 0", mode, first)).unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = v["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["provider"].as_str().unwrap().rsplit('.').next().unwrap(),
                    l["consumer"].as_str().unwrap().rsplit('.').next().unwrap(),
                    l["contract"].as_str().unwrap(),
                    l["level"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(rows, expected, "{text}");
    }
    for order in ["", "order = 1"] {
        std::fs::write(
            repo.join("a.tf"),
            route(
                "first",
                order,
                "match-all-conditions",
                condition("tags", "equals", "support:team-b"),
            ),
        )
        .unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["links"].as_array().unwrap().is_empty(), "{text}");
    }
}

#[test]
fn incident_json_block_forms_keep_native_spans_and_opentofu_shadowing() {
    let repo = fixture_repo(
        "alerts_incident_json",
        "alert-forwarding",
        Some("https://example.com/acme/json.git"),
    );
    std::fs::remove_file(repo.join("main.tf")).unwrap();
    let engine = Engine::open(&repo).unwrap();
    for arrays in [false, true] {
        let block = |v: serde_json::Value| if arrays { serde_json::json!([v]) } else { v };
        let monitor = serde_json::json!({"name":"JSON monitor", "tags":["target:team-c"]});
        let og = serde_json::json!({"team_id":"team-c-id", "criteria":block(serde_json::json!({"type":"match-all-conditions", "conditions":block(serde_json::json!({"field":"tags", "operation":"equals", "expected_value":"target:team-c"}))})), "notify":block(serde_json::json!({"type":"none"}))});
        let pd = serde_json::json!({"service":"checkout-id", "set":block(serde_json::json!({"id":"start", "rule":block(serde_json::json!({"condition":block(serde_json::json!({"expression":"event.custom_details.support matches 'team-b'"})), "actions":block(serde_json::json!({}))}))})), "catch_all":block(serde_json::json!({"actions":block(serde_json::json!({}))}))});
        let document = serde_json::json!({"resource":block(serde_json::json!({"datadog_monitor":block(serde_json::json!({"json":block(monitor)})), "opsgenie_team_routing_rule":block(serde_json::json!({"json":block(og)})), "pagerduty_event_orchestration_service":block(serde_json::json!({"json":block(pd)}))}))});
        let source = serde_json::to_string_pretty(&document).unwrap();
        std::fs::write(repo.join("native.tf.json"), &source).unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = v["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["contract"].as_str().unwrap(),
                    l["level"].as_str().unwrap(),
                    l["provider"].as_str().unwrap(),
                    l["consumer"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [
                (
                    "alert::support::team-b",
                    "exact",
                    "example.com/acme/json::rules.yaml::groups[0].rules[0]",
                    "example.com/acme/json::alertmanager.yaml::route.routes[0]"
                ),
                (
                    "alert::target::team-c",
                    "exact",
                    "example.com/acme/json::native.tf.json::datadog_monitor.json",
                    "example.com/acme/json::native.tf.json::opsgenie_team_routing_rule.json"
                ),
                (
                    "alert::target::team-c",
                    "exact",
                    "example.com/acme/json::rules.yaml::groups[0].rules[0]",
                    "example.com/acme/json::native.tf.json::opsgenie_team_routing_rule.json"
                ),
                (
                    "alert::support::team-b",
                    "exact",
                    "example.com/acme/json::rules.yaml::groups[0].rules[0]",
                    "example.com/acme/json::native.tf.json::pagerduty_event_orchestration_service.json.set[0].rule[0]"
                ),
            ],
            "{text}"
        );
        let rule = v["iac_nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["address"] == "pagerduty_event_orchestration_service.json.set[0].rule[0]")
            .unwrap();
        let line = source
            .lines()
            .position(|l| l.contains("\"expression\""))
            .unwrap()
            + 1;
        assert_eq!(rule["line"], line, "{text}");
        assert_eq!(rule["path"], "native.tf.json");
        std::fs::write(
            repo.join("native.tofu.json"),
            source.replace("target:team-c", "target:other"),
        )
        .unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["links"].as_array().unwrap().len(), 3, "{text}");
        assert!(
            v["links"]
                .as_array()
                .unwrap()
                .iter()
                .all(|l| !l["consumer"].as_str().unwrap().contains("native.tf.json")),
            "{text}"
        );
        std::fs::remove_file(repo.join("native.tofu.json")).unwrap();
    }
}

#[test]
fn dynamic_opsgenie_conjunctions_are_unknown_and_true_disjunctions_remain_proven() {
    let repo = fixture_repo(
        "alerts_dynamic_criteria",
        "alert-forwarding",
        Some("https://example.com/acme/dynamic.git"),
    );
    for path in ["rules.yaml", "alertmanager.yaml", "main.tf"] {
        std::fs::remove_file(repo.join(path)).unwrap();
    }
    let source = r#"resource "datadog_monitor" "cedar" {
  name = "Neutral dynamic-criteria control"
  type = "query alert"
  query = "sum(last_5m):sum:neutral.metric{*} > 1"
  message = "Neutral"
  tags = ["support:squad-elm"]
}
resource "opsgenie_team_routing_rule" "guarded" {
  team_id = "team-elm-id"
  criteria {
    type = "match-all-conditions"
    conditions {
      field = "tags"
      operation = "equals"
      expected_value = "support:squad-elm"
    }
    dynamic "conditions" {
      for_each = [1]
      content {
        field = "tags"
        operation = "equals"
        expected_value = "gate:closed"
      }
    }
  }
  notify { type = "none" }
}
"#;
    let engine = Engine::open(&repo).unwrap();
    for (source, matches) in [
        (source.to_string(), false),
        (
            source.replace("match-all-conditions", "match-any-condition"),
            true,
        ),
    ] {
        std::fs::write(repo.join("main.tf"), source).unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["providers"].as_array().unwrap().len(), 1, "{text}");
        assert_eq!(v["consumers"].as_array().unwrap().len(), 1, "{text}");
        let actual = v["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["contract"].as_str().unwrap(),
                    l["level"].as_str().unwrap(),
                    l["provider"].as_str().unwrap(),
                    l["consumer"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let expected = if matches {
            vec![(
                "alert::support::squad-elm",
                "exact",
                "example.com/acme/dynamic::main.tf::datadog_monitor.cedar",
                "example.com/acme/dynamic::main.tf::opsgenie_team_routing_rule.guarded",
            )]
        } else {
            Vec::new()
        };
        assert_eq!(actual, expected, "{text}");
    }
}

#[test]
fn unexpanded_opsgenie_rules_preserve_order_without_projecting_a_phantom_base() {
    let repo = fixture_repo(
        "alerts_order_barriers",
        "alert-forwarding",
        Some("https://example.com/acme/barriers.git"),
    );
    for path in ["rules.yaml", "alertmanager.yaml", "main.tf"] {
        std::fs::remove_file(repo.join(path)).unwrap();
    }
    std::fs::write(
        repo.join("monitors.tf"),
        r#"resource "datadog_monitor" "source" {
 name = "Order control"
 tags = ["support:squad-ash"]
}
"#,
    )
    .unwrap();
    let criteria = "criteria {\n type = \"match-all-conditions\"\n conditions {\n field = \"tags\"\n operation = \"equals\"\n expected_value = \"support:squad-ash\"\n }\n}\n";
    let route = |name: &str, meta: &str, team: &str, order: usize, criteria: &str| {
        format!(
            "resource \"opsgenie_team_routing_rule\" {name:?} {{\n {meta}\n team_id = {team}\n order = {order}\n {criteria}\n notify {{ type = \"none\" }}\n}}\n"
        )
    };
    std::fs::write(
        repo.join("second.tf"),
        route("second", "", "\"team-ash-id\"", 1, criteria),
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    for (meta, team, first_criteria, projected, expected_name) in [
        ("count = 1", "\"team-ash-id\"", criteria, false, None),
        (
            "for_each = {neutral = 1}",
            "\"team-ash-id\"",
            criteria,
            false,
            None,
        ),
        ("", "\"team-ash-id\"", "", false, None),
        ("", "var.team.id", criteria, true, None),
        (
            "count = 1",
            "\"another-team-id\"",
            criteria,
            false,
            Some("second"),
        ),
        ("", "\"team-ash-id\"", criteria, true, Some("first")),
    ] {
        std::fs::write(
            repo.join("first.tf"),
            route("first", meta, team, 0, first_criteria),
        )
        .unwrap();
        let text = engine.export().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let first = "example.com/acme/barriers::first.tf::opsgenie_team_routing_rule.first";
        assert_eq!(
            v["consumers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["site"] == first),
            projected,
            "{text}"
        );
        assert!(
            !v["iac_nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n["identity"] == first
                    && n["type"] == "OpsgenieRoutingRule"
                    && !projected),
            "{text}"
        );
        let actual = v["links"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["contract"].as_str().unwrap(),
                    l["level"].as_str().unwrap(),
                    l["provider"].as_str().unwrap(),
                    l["consumer"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let site = expected_name.map(|name| {
            format!("example.com/acme/barriers::{name}.tf::opsgenie_team_routing_rule.{name}")
        });
        let expected = site
            .as_deref()
            .map(|site| {
                vec![(
                    "alert::support::squad-ash",
                    "exact",
                    "example.com/acme/barriers::monitors.tf::datadog_monitor.source",
                    site,
                )]
            })
            .unwrap_or_default();
        assert_eq!(actual, expected, "{meta}: {text}");
    }
}
