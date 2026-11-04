//! Config values (#639, ADR 0025): every scalar of the YAML and JSON files the walk visits, and
//! of the CI configuration under the hidden CI paths, found by `search` with the kind `value`.

use std::fs;

use meada::engine::{self, Engine};

use crate::common::{assert_golden, fixture_repo};

const ORIGIN: Option<&str> = Some("https://example.com/acme/app.git");

/// The `search` answers of `queries` with the kind `value`, in order, a blank line between two.
fn searched(engine: &Engine, queries: &[&str]) -> String {
    (queries.iter())
        .map(|q| engine.search(q, Some("value"), None).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fixture's Helm values carry #638's labels, its multi-document Kubernetes file a Secret,
/// `.github/workflows/ci.yml` a workflow; the editor's hidden `.vscode/` and the
/// `.meadaignore`d `secrets/` stay out, the lockfile and the Helm template that does not parse
/// are skipped. A query matches values and key paths, exact first, then prefix, then substring.
#[test]
fn search_with_the_kind_value_finds_config_values_by_value_or_key_path() {
    let repo = fixture_repo("config_values_search", "config-values", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let queries = [
        "team-b",
        "support",
        "ghcr.io/acme/app",
        "password",
        "app.kubernetes.io",
        "description",
        "tags",
        "7357",
        "kept-out-by-meadaignore",
        "lockfileversion",
        "replicas",
        "flags",
    ];
    assert_golden("config-values.search.txt", &searched(&engine, &queries));
}

/// `meada status` counts the config files read and skipped (the lockfile, the Helm template
/// that does not parse) and the values redacted.
#[test]
fn status_counts_the_config_files_read_and_skipped_and_the_values_redacted() {
    let repo = fixture_repo("config_values_status", "config-values", ORIGIN);
    Engine::open(&repo).unwrap().export().unwrap();
    assert_golden("config-values.status.txt", &engine::status(&repo).unwrap());
}

/// With no kind, values answer only when no symbol and no contract matches: `health` finds the
/// function alone though `HealthCheckFailing` is a value, and `team-b` finds the values.
#[test]
fn the_default_search_falls_back_to_values_only_when_nothing_else_matches() {
    let repo = fixture_repo("config_values_default", "config-values", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let health = engine.search("health", None, None).unwrap();
    assert_eq!(
        health,
        engine.search("health", Some("symbol"), None).unwrap()
    );
    assert!(health.contains("healthCheck"), "{health}");
    assert!(!health.contains("HealthCheckFailing"), "{health}");
    assert!(
        (engine
            .search("HealthCheckFailing", Some("value"), None)
            .unwrap())
        .contains("alert"),
        "the value is indexed"
    );
    let team = engine.search("team-b", None, None).unwrap();
    assert_eq!(team, engine.search("team-b", Some("value"), None).unwrap());
    assert!(team.contains("chart/values-prd.yaml:6"), "{team}");
    let nothing = engine.search("no-such-value", None, None).unwrap();
    assert_eq!(nothing, "no matches for `no-such-value`\n");
}

/// Values stay out of `meada export`, and a Secret's values out of the index itself: its file
/// holds neither the encoded `data` nor the plain `stringData`.
#[test]
fn values_stay_out_of_the_export_and_a_secret_out_of_the_index() {
    let repo = fixture_repo("config_values_export", "config-values", ORIGIN);
    let export = Engine::open(&repo).unwrap().export().unwrap();
    for value in [
        "team-b",
        "ghcr.io",
        "HealthCheckFailing",
        "config/settings.json",
    ] {
        assert!(!export.contains(value), "{value} in the export");
    }
    let index = fs::read(repo.join(".meada/index.db")).unwrap();
    let found = |needle: &[u8]| index.windows(needle.len()).any(|w| w == needle);
    assert!(found(b"team-b"), "the index holds the values");
    for secret in [&b"c2VjcmV0LXBhc3N3b3Jk"[..], b"app-admin"] {
        assert!(!found(secret), "{}", String::from_utf8_lossy(secret));
    }
}
