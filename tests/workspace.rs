mod common;

use std::fs;
use std::path::PathBuf;

use common::{assert_golden, fixture_repo};
use meada::engine::{self, Engine};

/// The two-repository fixture as siblings in one case directory: `http-web` consumes from
/// `http-api` and names a peer that is not cloned.
fn web_and_api(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(case, "http-web", Some("git@example.com:acme/web.git")),
        fixture_repo(case, "http-api", Some("https://example.com/acme/api.git")),
    )
}

#[test]
fn consumer_export_links_fetch_calls_to_peer_routes() {
    let (web, _) = web_and_api("http_consumer_export");

    let export = Engine::open(&web).unwrap().export().unwrap();

    assert_golden("http-web.export.json", &export);
}

#[test]
fn provider_export_shows_links_from_sibling_consumers() {
    let (_, api) = web_and_api("http_provider_export");

    let export = Engine::open(&api).unwrap().export().unwrap();

    assert_golden("http-api.export.json", &export);
}

#[test]
fn search_finds_contracts_across_the_workspace() {
    let (web, _) = web_and_api("http_search");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "http-web.search-contract.txt",
        &engine
            .search("HTTP::GET::/api/users", Some("contract"), None)
            .unwrap(),
    );
}

#[test]
fn search_finds_symbols_across_the_workspace() {
    let (web, _) = web_and_api("http_search_symbols");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "http-web.search-symbol.txt",
        &engine.search("query", Some("symbol"), None).unwrap(),
    );
}

#[test]
fn search_ranks_a_route_path_against_the_path_of_each_contract() {
    let (web, _) = web_and_api("http_search_route");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "http-web.search-route.txt",
        &engine.search("/api/users", Some("route"), None).unwrap(),
    );
}

#[test]
fn search_rejects_an_unknown_kind_naming_the_valid_ones() {
    let (web, _) = web_and_api("http_search_kind");
    let engine = Engine::open(&web).unwrap();

    let error = engine.search("users", Some("file"), None).unwrap_err();

    assert_eq!(
        error.to_string(),
        "`kind` takes `symbol`, `contract`, `route`, `resource`, `channel`, `document`, `ambiguous` or `unresolved`, not `file`"
    );
}

#[test]
fn context_opens_a_symbol_of_a_peer() {
    let (web, _) = web_and_api("http_context_peer");
    let engine = Engine::open(&web).unwrap();

    assert_golden(
        "http-web.context-peer.txt",
        &engine.context("lib/db.ts::query", None).unwrap(),
    );
}

#[test]
fn status_lists_the_workspace_index_states_and_missing_peers() {
    let (web, api) = web_and_api("http_status");

    assert_golden(
        "http-web.status-unindexed.txt",
        &engine::status(&web).unwrap(),
    );
    // The consumer's first query builds the peer's index inside the peer's own `.meada/`.
    Engine::open(&web).unwrap().refresh().unwrap();
    assert_golden("http-web.status.txt", &engine::status(&web).unwrap());
    assert_eq!(
        common::git(&api, &["status", "--porcelain", "--ignored"]),
        "!! .meada/\n"
    );
}

#[test]
fn a_peer_without_remote_is_found_by_its_path_hint() {
    let web = fixture_repo("path_hint_web", "http-web", None);
    fixture_repo("path_hint_api", "http-api", None);
    fs::write(
        web.join("meada.toml"),
        "[[peer]]\nrepo = \"local/http-api\"\npath = \"../../path_hint_api/http-api\"\n",
    )
    .unwrap();

    assert_eq!(
        engine::status(&web).unwrap(),
        "local/http-web (.) not indexed\nlocal/http-api (../../path_hint_api/http-api) not indexed\n"
    );
}

#[test]
fn a_peer_without_remote_is_not_found_among_siblings() {
    let case = "local_sibling";
    let web = fixture_repo(case, "http-web", None);
    fixture_repo(case, "http-api", None);
    fs::write(
        web.join("meada.toml"),
        "[[peer]]\nrepo = \"local/http-api\"\n",
    )
    .unwrap();

    assert_eq!(
        engine::status(&web).unwrap(),
        "local/http-web (.) not indexed\nmissing peer: local/http-api, named by local/http-web\n"
    );
}

/// The error `Engine::open` gives for a repository whose `meada.toml` is `toml`, with the
/// repository's path replaced by `<repo>`.
fn config_error(case: &str, toml: &str) -> String {
    let repo = fixture_repo(case, "ts-basic", Some("https://example.com/acme/web.git"));
    fs::write(repo.join("meada.toml"), toml).unwrap();
    let error = Engine::open(&repo)
        .err()
        .expect("an invalid meada.toml fails")
        .to_string();
    error.replace(repo.to_str().unwrap(), "<repo>")
}

#[test]
fn invalid_config_errors_name_file_line_and_key() {
    for (case, toml, expected) in [
        (
            "config_serve_key",
            "[serve]\nhosts = [\"api.example.com\"]\ndomains = [\"openapi.yaml\"]\n",
            "<repo>/meada.toml:3: `serve.domains` is not a meada.toml key",
        ),
        (
            "config_documents_absolute",
            "[serve]\ndocuments = [\"docs/api.yaml\", \"/etc/api.yaml\"]\n",
            "<repo>/meada.toml:2: `serve.documents`: `/etc/api.yaml` is not a path inside the repository",
        ),
        (
            "config_documents_parent",
            "[serve]\ndocuments = [\"docs/../../api.yaml\"]\n",
            "<repo>/meada.toml:2: `serve.documents`: `docs/../../api.yaml` is not a path inside the repository",
        ),
        (
            "config_resource_key",
            "[[resource]]\nkind = \"postgres\"\nid = \"orders\"\nrole = \"read\"\nschema = \"public\"\n",
            "<repo>/meada.toml:5: `resource.schema` is not a meada.toml key",
        ),
        (
            "config_resource_kind",
            "[[resource]]\nkind = \"Postgres\"\nid = \"orders\"\nrole = \"read\"\n",
            "<repo>/meada.toml:2: `resource.kind`: `Postgres` is not a lowercase identifier \
             (ASCII letters, digits and hyphens, starting with a letter)",
        ),
        (
            "config_resource_role",
            "[[resource]]\nkind = \"postgres\"\nid = \"orders\"\nrole = \"append\"\n",
            "<repo>/meada.toml:4: `resource.role`: unknown variant `append`, \
             expected one of `read`, `write`, `read-write`",
        ),
        (
            "config_resource_id",
            "[[resource]]\nkind = \"postgres\"\nid = \"\"\nrole = \"read\"\n",
            "<repo>/meada.toml:3: `resource.id`: is empty",
        ),
        (
            "config_prefix",
            "[serve]\nhosts = [\"api.example.com\"]\nprefix = \"api\"\n",
            "<repo>/meada.toml:3: `serve.prefix`: `api` must start with `/`",
        ),
        (
            "config_strip",
            "[serve]\nstrip = \"v1/\"\n",
            "<repo>/meada.toml:2: `serve.strip`: `v1/` must start with `/`",
        ),
        (
            "config_unknown",
            "[[peer]]\nrepo = \"example.com/acme/api\"\nbranch = \"main\"\n",
            "<repo>/meada.toml:3: `peer.branch` is not a meada.toml key",
        ),
        (
            "config_missing",
            "[[peer]]\npath = \"../api\"\n",
            "<repo>/meada.toml:1: `peer.repo` is missing",
        ),
        (
            "config_type",
            "[serve]\nhosts = [\n  \"api.example.com\",\n  443,\n]\n",
            "<repo>/meada.toml:4: `serve.hosts`: invalid type: integer `443`, expected a string",
        ),
        (
            "config_syntax",
            "[[peer]\n",
            "<repo>/meada.toml:1: unclosed array table, expected `]`",
        ),
    ] {
        assert_eq!(config_error(case, toml), expected, "{case}");
    }
}
