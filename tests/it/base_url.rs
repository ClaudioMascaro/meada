//! Base URL declarations (`[[base_url]]` in `meada.toml`): a consumer whose base URL comes from
//! a declared variable links, `declared`, to the routes served under the declared directory of
//! its own repository.

use std::path::{Path, PathBuf};

use crate::common::fixture_repo;
use meada::engine::Engine;
use serde_json::Value;

const MONO: &str = "example.com/acme/mono";

/// `tests/fixtures/base-url-mono`, as `example.com/acme/mono`.
fn mono(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "base-url-mono",
        Some("https://example.com/acme/mono.git"),
    )
}

/// `mono` and `base-url-peer` as sibling clones; the mono's `meada.toml` keeps its
/// `[[base_url]]` and names the peer, so both are in the workspace.
fn with_peer(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "base-url-peer",
        Some("https://example.com/acme/peer.git"),
    );
    let repo = mono(case);
    let declared = std::fs::read_to_string(repo.join("meada.toml")).unwrap();
    let peer = "\n[[peer]]\nrepo = \"example.com/acme/peer\"\n";
    std::fs::write(repo.join("meada.toml"), declared + peer).unwrap();
    repo
}

/// The HTTP links of the workspace `repo` opens.
fn links(repo: &Path) -> Vec<Value> {
    let exported = Engine::open(repo).unwrap().export().unwrap();
    let data: Value = serde_json::from_str(&exported).unwrap();
    let links = data["links"].as_array().unwrap().iter();
    links
        .filter(|l| l["contract"].as_str().unwrap().starts_with("http::"))
        .cloned()
        .collect()
}

/// The links whose consumer is a call of `site` (`<path>::<function>`).
fn of<'a>(links: &'a [Value], site: &str) -> Vec<&'a Value> {
    let site = format!("::{site}::");
    let consumer = |l: &&Value| l["consumer"].as_str().unwrap().contains(&site);
    links.iter().filter(consumer).collect()
}

/// `(level, provider, reason)` of each link.
fn shown(links: &[&Value]) -> Vec<(String, String, String)> {
    let text = |v: &Value| v.as_str().unwrap().to_string();
    let row = |l: &&Value| (text(&l["level"]), text(&l["provider"]), text(&l["reason"]));
    links.iter().map(row).collect()
}

fn declared(reason: &str) -> (String, String, String) {
    (
        "declared".into(),
        format!("{MONO}::svc-b/main.go::run"),
        format!("{reason}, declared for svc-b/ in meada.toml; served by {MONO}"),
    )
}

#[test]
fn a_declared_variable_links_only_inside_its_directory() {
    let found = links(&mono("base_url_dir"));
    assert_eq!(
        shown(&of(&found, "web/src/api.ts::runJob")),
        [declared("base URL from B_URL")],
        "{found:#?}"
    );
}

#[test]
fn without_the_declaration_the_link_is_what_it_was() {
    let repo = mono("base_url_undeclared");
    std::fs::remove_file(repo.join("meada.toml")).unwrap();
    let found = links(&repo);
    // The base binary's answer: the compose host's route, inferred.
    let run = shown(&of(&found, "web/src/api.ts::runJob"));
    assert_eq!(run.len(), 1, "{found:#?}");
    assert_eq!(run[0].0, "inferred");
    assert_eq!(run[0].1, format!("{MONO}::svc-c/main.go::run"));
    assert!(run[0].2.starts_with("Compose service"), "{run:?}");
    assert!(found.iter().all(|l| l["level"] != "declared"), "{found:#?}");
}

#[test]
fn a_directory_is_not_a_prefix_of_another() {
    // `GET /status` is served only under `svc-b-ui/`.
    let found = links(&mono("base_url_prefix"));
    assert_eq!(of(&found, "web/src/api.ts::status"), Vec::<&Value>::new());
}

#[test]
fn a_declared_variable_keeps_the_proxy_endpoint_in_its_reason() {
    let found = links(&mono("base_url_proxy"));
    assert_eq!(
        shown(&of(&found, "web/src/front.ts::viaProxy")),
        [declared(
            "base URL from B_URL through the proxy endpoint /b"
        )],
        "{found:#?}"
    );
}

#[test]
fn the_declaration_wins_over_an_infrastructure_value() {
    // The compose file gives `web` `B_URL: http://svc-c:8080`.
    let repo = mono("base_url_compose");
    let compose = std::fs::read_to_string(repo.join("docker-compose.yml")).unwrap();
    assert!(compose.contains("B_URL: http://svc-c:8080"));
    let found = links(&repo);
    assert_eq!(
        shown(&of(&found, "web/src/api.ts::runJob")),
        [declared("base URL from B_URL")]
    );
}

#[test]
fn the_declaration_wins_over_a_peer_naming_the_same_variable() {
    let repo = mono("base_url_over_peer");
    let declared_only = std::fs::read_to_string(repo.join("meada.toml")).unwrap();
    let peer = "\n[[peer]]\nrepo = \"github.com/acme/other\"\nbase_url_env = [\"B_URL\"]\n";
    std::fs::write(repo.join("meada.toml"), declared_only + peer).unwrap();
    let found = links(&repo);
    assert_eq!(
        shown(&of(&found, "web/src/api.ts::runJob")),
        [declared("base URL from B_URL")]
    );
}

#[test]
fn a_literal_host_keeps_its_rule() {
    let found = links(&mono("base_url_literal"));
    assert_eq!(
        of(&found, "web/src/api.ts::literalHost"),
        Vec::<&Value>::new()
    );
}

#[test]
fn a_declared_gateway_match_stays_inferred() {
    let repo = mono("base_url_gateway");
    let declared_only = std::fs::read_to_string(repo.join("meada.toml")).unwrap();
    std::fs::write(
        repo.join("meada.toml"),
        format!("[serve]\nprefix = \"/api\"\n\n{declared_only}"),
    )
    .unwrap();
    let found = links(&repo);
    assert_eq!(
        shown(&of(&found, "web/src/api.ts::gatewayRun")),
        [(
            "inferred".into(),
            format!("{MONO}::svc-b/main.go::run"),
            "gateway prefix /api".into()
        )],
        "{found:#?}"
    );
}

#[test]
fn a_declaration_reaches_only_its_own_repository() {
    let declared = links(&with_peer("base_url_own_declared"));
    let repo = with_peer("base_url_own_undeclared");
    let text = std::fs::read_to_string(repo.join("meada.toml")).unwrap();
    let peer_only = text.replace("[[base_url]]\nenv = [\"B_URL\"]\ndir = \"svc-b\"\n", "");
    assert_ne!(peer_only, text);
    std::fs::write(repo.join("meada.toml"), peer_only).unwrap();
    let undeclared = links(&repo);
    let peer_run = "src/peer.ts::peerRun";
    assert!(!of(&undeclared, peer_run).is_empty(), "{undeclared:#?}");
    assert_eq!(
        shown(&of(&declared, peer_run)),
        shown(&of(&undeclared, peer_run))
    );
}
