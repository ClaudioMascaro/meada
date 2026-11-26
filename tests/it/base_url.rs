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
        format!("{MONO}::svc-b/routes.go::Routes.run"),
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
            format!("{MONO}::svc-b/routes.go::Routes.run"),
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
    let declaration = "[[base_url]]\nenv = [\"B_URL\", \"B_BASE_URL\"]\ndir = \"svc-b\"\n";
    let peer_only = text.replace(declaration, "");
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

#[test]
fn a_relative_url_in_a_package_that_serves_it_links_there() {
    let found = links(&mono("base_url_relative"));
    let test = shown(&of(&found, "svc-b/router_test.go::TestRun"));
    assert_eq!(test.len(), 1, "{found:#?}");
    assert_eq!(test[0].0, "inferred");
    assert_eq!(test[0].1, format!("{MONO}::svc-b/routes.go::Routes.run"));
    assert!(!test[0].2.contains("providers fit"), "{test:?}");
}

#[test]
fn a_relative_url_its_package_does_not_serve_keeps_every_fit() {
    let found = links(&mono("base_url_relative_web"));
    let web = shown(&of(&found, "web/src/client.ts::relativeRun"));
    let providers: Vec<&str> = web.iter().map(|(_, p, _)| p.as_str()).collect();
    assert_eq!(
        providers,
        [
            format!("{MONO}::svc-b/routes.go::Routes.run"),
            format!("{MONO}::svc-c/main.go::run")
        ],
        "{found:#?}"
    );
    assert!(web.iter().all(|(level, ..)| level == "ambiguous"));
}

#[test]
fn a_relative_url_drops_other_repositories_too() {
    let found = links(&with_peer("base_url_relative_peer"));
    // The peer serves `POST /run` too.
    assert!(
        found.iter().any(|l| l["provider"]
            .as_str()
            .unwrap()
            .starts_with("example.com/acme/peer::")),
        "{found:#?}"
    );
    let test = shown(&of(&found, "svc-b/router_test.go::TestRun"));
    let providers: Vec<&str> = test.iter().map(|(_, p, _)| p.as_str()).collect();
    assert_eq!(
        providers,
        [format!("{MONO}::svc-b/routes.go::Routes.run")],
        "{found:#?}"
    );
}

#[test]
fn the_go_client_links_each_route_declared() {
    let found = links(&mono("base_url_go_client"));
    let reason =
        format!("base URL from B_BASE_URL, declared for svc-b/ in meada.toml; served by {MONO}");
    let at = |site: &str, provider: &str| {
        let row = (
            "declared".to_string(),
            format!("{MONO}::{provider}"),
            reason.clone(),
        );
        assert_eq!(shown(&of(&found, site)), [row], "{found:#?}");
    };
    at(
        "svc-a/client/client.go::Client.Run",
        "svc-b/routes.go::Routes.run",
    );
    at(
        "svc-a/client/client.go::Client.Cancel",
        "svc-b/routes.go::Routes.cancel",
    );
}

#[test]
fn without_the_declaration_the_go_client_is_not_declared() {
    let repo = mono("base_url_go_undeclared");
    std::fs::remove_file(repo.join("meada.toml")).unwrap();
    let found = links(&repo);
    let run = shown(&of(&found, "svc-a/client/client.go::Client.Run"));
    let providers: Vec<&str> = run.iter().map(|(_, p, _)| p.as_str()).collect();
    assert_eq!(
        providers,
        [
            format!("{MONO}::svc-b/routes.go::Routes.run"),
            format!("{MONO}::svc-c/main.go::run")
        ],
        "{found:#?}"
    );
    for (level, _, reason) in &run {
        assert_eq!(level, "ambiguous");
        let why = "2 providers fit; base URL from B_BASE_URL, no peer's base_url_env; ";
        assert!(reason.starts_with(why), "{reason}");
    }
    let cancel = shown(&of(&found, "svc-a/client/client.go::Client.Cancel"));
    assert_eq!(cancel.len(), 1, "{found:#?}");
    assert_eq!(cancel[0].0, "inferred");
    assert_eq!(
        cancel[0].1,
        format!("{MONO}::svc-b/routes.go::Routes.cancel")
    );
}

#[test]
fn a_request_struct_change_reaches_both_consumers() {
    let repo = mono("base_url_lock");
    let file = repo.join("svc-b/svc/types.go");
    let text = std::fs::read_to_string(&file).unwrap();
    let renamed = text.replace("`json:\"name\"`", "`json:\"job\"`");
    assert_ne!(renamed, text);
    std::fs::write(&file, renamed).unwrap();
    crate::common::git(&repo, &["add", "-A"]);
    crate::common::git(&repo, &["commit", "-q", "-m", "rename"]);
    let changes = meada::engine::Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = Engine::open(&repo).unwrap().affected(changes).unwrap();
    let answer = serde_json::to_value(answer).unwrap();
    for path in ["svc-a/client/client.go", "web/src/api.ts"] {
        let entry = (answer["files"].as_array().into_iter().flatten())
            .find(|f| f["path"] == path)
            .unwrap_or_else(|| panic!("{path}: {answer:#}"));
        let certain = ["exact", "declared"].contains(&entry["level"].as_str().unwrap());
        assert!(certain, "{path}: {entry}");
    }
}

/// A `_test.go` file of the client's package: a helper builds the client on a test server's
/// literal URL, and `probe` reads the client's base itself.
const CLIENT_TEST: &str = r#"package client

import "net/http"

func newTestClient() *Client { return New("http://127.0.0.1:8080") }

func probe(c *Client) { _, _ = http.Post(c.base+"/run", "application/json", nil) }
"#;

/// `site` has no link: its base URL is unknown, so its call has no contract.
fn assert_no_base(found: &[Value], site: &str) {
    assert_eq!(shown(&of(found, site)), [], "{site}");
}

#[test]
fn a_test_helper_does_not_feed_the_client_base() {
    let repo = mono("base_url_test_helper");
    std::fs::write(repo.join("svc-a/client/client_test.go"), CLIENT_TEST).unwrap();
    let found = links(&repo);
    let reason =
        format!("base URL from B_BASE_URL, declared for svc-b/ in meada.toml; served by {MONO}");
    for (site, provider) in [
        ("svc-a/client/client.go::Client.Run", "Routes.run"),
        ("svc-a/client/client.go::Client.Cancel", "Routes.cancel"),
    ] {
        let row = (
            "declared".to_string(),
            format!("{MONO}::svc-b/routes.go::{provider}"),
            reason.clone(),
        );
        assert_eq!(shown(&of(&found, site)), [row], "{found:#?}");
    }
}

#[test]
fn a_field_only_test_files_construct_stays_unknown() {
    let repo = mono("base_url_test_only");
    std::fs::remove_file(repo.join("svc-a/main.go")).unwrap();
    let helper = r#"package client

import "example.com/mono/svc-a/config"

func newClient() *Client {
	cfg, _ := config.Load()
	return New(cfg.B.BaseURL)
}
"#;
    std::fs::write(repo.join("svc-a/client/client_test.go"), helper).unwrap();
    let found = links(&repo);
    assert_no_base(&found, "svc-a/client/client.go::Client.Run");
    assert_no_base(&found, "svc-a/client/client.go::Client.Cancel");
}

#[test]
fn a_read_inside_a_test_file_keeps_every_caller() {
    let repo = mono("base_url_test_read");
    std::fs::write(repo.join("svc-a/client/client_test.go"), CLIENT_TEST).unwrap();
    let found = links(&repo);
    assert_no_base(&found, "svc-a/client/client_test.go::probe");
}

#[test]
fn a_declared_link_never_leaves_its_repository() {
    let repo = with_peer("base_url_peer_dir");
    // The peer serves `POST /run` under a directory named like the declared one.
    let peer = repo.parent().unwrap().join("base-url-peer");
    crate::common::git(&peer, &["mv", "api", "svc-b"]);
    crate::common::git(&peer, &["commit", "-q", "-m", "svc-b"]);
    let found = links(&repo);
    let in_peer = |l: &Value| {
        let provider = l["provider"].as_str().unwrap();
        provider.starts_with("example.com/acme/peer::svc-b/")
    };
    assert!(found.iter().any(in_peer), "{found:#?}");
    assert_eq!(
        shown(&of(&found, "web/src/api.ts::runJob")),
        [declared("base URL from B_URL")],
        "{found:#?}"
    );
}

#[test]
fn a_variable_base_url_keeps_every_fit_its_own_package_serves_too() {
    let repo = mono("base_url_variable_own");
    let caller = r#"package main

import (
	"net/http"
	"os"
)

func callRun() { _, _ = http.Post(os.Getenv("RUN_URL")+"/run", "application/json", nil) }
"#;
    std::fs::write(repo.join("svc-b/caller.go"), caller).unwrap();
    let found = links(&repo);
    let run = shown(&of(&found, "svc-b/caller.go::callRun"));
    let providers: Vec<&str> = run.iter().map(|(_, p, _)| p.as_str()).collect();
    assert_eq!(
        providers,
        [
            format!("{MONO}::svc-b/routes.go::Routes.run"),
            format!("{MONO}::svc-c/main.go::run")
        ],
        "{found:#?}"
    );
    for (level, _, reason) in &run {
        assert_eq!(level, "ambiguous");
        let why = "2 providers fit; base URL from RUN_URL, no peer's base_url_env; ";
        assert!(reason.starts_with(why), "{reason}");
    }
}

/// A `_test.go` file of another package that writes a field named like the client's base.
const OTHER_TEST: &str = r#"package main

import "testing"

type other struct{ base string }

func TestOther(t *testing.T) {
	o := &other{}
	o.base = "http://127.0.0.1:1"
	_ = o
}
"#;

/// Run and Cancel of the Go client link `declared` to their routes.
fn assert_client_declared(found: &[Value]) {
    let reason =
        format!("base URL from B_BASE_URL, declared for svc-b/ in meada.toml; served by {MONO}");
    for (site, provider) in [
        ("svc-a/client/client.go::Client.Run", "Routes.run"),
        ("svc-a/client/client.go::Client.Cancel", "Routes.cancel"),
    ] {
        let row = (
            "declared".to_string(),
            format!("{MONO}::svc-b/routes.go::{provider}"),
            reason.clone(),
        );
        assert_eq!(shown(&of(found, site)), [row], "{found:#?}");
    }
}

#[test]
fn a_test_file_write_elsewhere_does_not_cancel_the_client_base() {
    let repo = mono("base_url_test_write");
    std::fs::write(repo.join("svc-b/other_test.go"), OTHER_TEST).unwrap();
    assert_client_declared(&links(&repo));
}

#[test]
fn a_test_file_write_still_cancels_a_read_in_a_test_file() {
    let repo = mono("base_url_test_write_read");
    std::fs::write(repo.join("svc-b/other_test.go"), OTHER_TEST).unwrap();
    let probe = r#"package client

import "net/http"

func probe(c *Client) { _, _ = http.Post(c.base+"/run", "application/json", nil) }
"#;
    std::fs::write(repo.join("svc-a/client/probe_test.go"), probe).unwrap();
    let found = links(&repo);
    assert_no_base(&found, "svc-a/client/probe_test.go::probe");
}

#[test]
fn a_function_local_type_elsewhere_does_not_cancel_the_client_base() {
    let repo = mono("base_url_local_type");
    let local = "package main\n\nfunc local() {\n\ttype pair struct{ a, b string }\n\t_ = pair{\"x\", \"y\"}\n}\n";
    std::fs::write(repo.join("svc-b/local.go"), local).unwrap();
    assert_client_declared(&links(&repo));
}

#[test]
fn a_literal_of_a_type_that_binds_to_nothing_keeps_the_base_unknown() {
    // Unknown, qualified or not: it may be the client's struct.
    for (case, ty) in [
        ("base_url_unbound_qualified", "missing.Pair"),
        ("base_url_unbound", "Missing"),
    ] {
        let repo = mono(case);
        let unbound = format!("package main\n\nfunc unbound() {{ _ = {ty}{{\"x\", \"y\"}} }}\n");
        std::fs::write(repo.join("svc-b/unbound.go"), unbound).unwrap();
        let found = links(&repo);
        assert_no_base(&found, "svc-a/client/client.go::Client.Run");
        assert_no_base(&found, "svc-a/client/client.go::Client.Cancel");
    }
}

fn assert_shadowed_client_has_no_base(case: &str, body: &str) {
    let repo = mono(case);
    let source = format!("package client\n\nfunc other() {{ {body} }}\n");
    std::fs::write(repo.join("svc-a/client/other.go"), source).unwrap();
    let found = links(&repo);
    assert_no_base(&found, "svc-a/client/client.go::Client.Run");
    assert_no_base(&found, "svc-a/client/client.go::Client.Cancel");
}

#[test]
fn a_local_name_declared_later_keeps_the_package_literal() {
    let repo = mono("base_url_shadow_later");
    let file = repo.join("svc-a/client/client.go");
    let client = std::fs::read_to_string(&file).unwrap();
    let other = r#"func other() { _ = Client{"http://x", nil}; var Client struct{}; _ = Client }"#;
    std::fs::write(file, client + "\n" + other).unwrap();
    let found = links(&repo);
    assert_no_base(&found, "svc-a/client/client.go::Client.Run");
    assert_no_base(&found, "svc-a/client/client.go::Client.Cancel");
}

#[test]
fn a_local_name_in_a_sibling_block_keeps_the_package_literal() {
    assert_shadowed_client_has_no_base(
        "base_url_shadow_sibling",
        r#"{ var Client struct{}; _ = Client }; { _ = Client{"http://x", nil} }"#,
    );
}

#[test]
fn the_tagged_field_competes_with_the_positional_initializer() {
    let repo = mono("base_url_initializer_control");
    let src = r#"package main

import "net/http"

type client struct{ baseURL string }
func New(c Config) *client { return &client{baseURL: c.BaseURL} }
func (c *client) send() { _, _ = http.Post(c.baseURL+"/run", "application/json", nil) }
"#;
    let config = "package main\ntype Config struct{ BaseURL string `env:\"A\"` }\n";
    std::fs::write(repo.join("svc-b/config.go"), config).unwrap();
    let manifest = repo.join("meada.toml");
    let manifest_text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        manifest,
        manifest_text + "\n[[base_url]]\nenv = [\"A\"]\ndir = \"svc-b\"\n",
    )
    .unwrap();
    let site = "svc-b/local.go::client.send";
    let file = repo.join("svc-b/local.go");
    std::fs::write(&file, src).unwrap();
    assert_eq!(
        shown(&of(&links(&repo), site)),
        [declared("base URL from A")]
    );
    let other = "func other() { client := client{\"http://x\"}; _ = client }\n";
    std::fs::write(file, src.to_owned() + other).unwrap();
    assert_no_base(&links(&repo), site);
}

#[test]
fn a_local_name_in_a_nested_initializer_keeps_the_package_literal() {
    assert_shadowed_client_has_no_base(
        "base_url_shadow_nested",
        r#"{ Client := Client{"http://x", nil}; _ = Client }"#,
    );
}

#[test]
fn a_local_name_can_hide_a_dot_imported_type() {
    let repo = mono("base_url_shadow_dot_import");
    let source = r#"package client
import . "image"
func other() { { Point := Point{1, 2}; _ = Point } }
"#;
    std::fs::write(repo.join("svc-a/client/other.go"), source).unwrap();
    assert_no_base(&links(&repo), "svc-a/client/client.go::Client.Run");
}

#[test]
fn a_positional_literal_with_several_type_definitions_keeps_the_base_unknown() {
    let repo = mono("base_url_several_types");
    // Go selects one legacy build tag. Both definitions are visible to the index.
    for (file, tag) in [("first.go", "variant"), ("second.go", "!variant")] {
        let source = format!("// +build {tag}\n\npackage main\ntype pair struct {{ n int }}\n");
        std::fs::write(repo.join("svc-b").join(file), source).unwrap();
    }
    let literal = "package main\nfunc other() { _ = pair{1} }\n";
    std::fs::write(repo.join("svc-b/other.go"), literal).unwrap();
    assert_no_base(&links(&repo), "svc-a/client/client.go::Client.Run");
}

#[test]
fn a_qualified_literal_with_a_local_head_keeps_the_base_unknown() {
    let repo = mono("base_url_local_qualifier");
    let source = r#"package main
import "image"
func other() { { var image struct{}; _ = image }; _ = image.Point{1, 2} }
"#;
    std::fs::write(repo.join("svc-b/other.go"), source).unwrap();
    assert_no_base(&links(&repo), "svc-a/client/client.go::Client.Run");
}

#[test]
fn a_test_file_write_still_counts_for_a_resty_request_field() {
    let repo = mono("base_url_test_request_write");
    let source = r#"package client
import (
    "os"
    "github.com/go-resty/resty/v2"
)
type sender struct { req *resty.Request }
func newSender() *sender { return &sender{req: resty.New().SetBaseURL(os.Getenv("B_URL")).R()} }
func (s *sender) send() { _, _ = s.req.Post("/run") }
"#;
    std::fs::write(repo.join("svc-a/client/sender.go"), source).unwrap();
    let site = "svc-a/client/sender.go::sender.send";
    assert_eq!(
        shown(&of(&links(&repo), site)),
        [declared("base URL from B_URL")]
    );

    let write = "package client\nfunc clear(s *sender) { s.req = nil }\n";
    std::fs::write(repo.join("svc-a/client/sender_test.go"), write).unwrap();
    assert_no_base(&links(&repo), site);
}
