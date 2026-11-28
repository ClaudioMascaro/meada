use crate::common::fixture_repo;
use meada::engine::Engine;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixture(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "ts-http-bases-peer",
        Some("https://example.com/acme/peer.git"),
    );
    fixture_repo(
        case,
        "ts-http-bases",
        Some("https://example.com/acme/client.git"),
    )
}

fn replace(repo: &Path, file: &str, from: &str, to: &str) {
    let path = repo.join(file);
    let src = std::fs::read_to_string(&path).unwrap();
    assert_eq!(src.matches(from).count(), 1, "{path:?}: {from}");
    std::fs::write(path, src.replace(from, to)).unwrap();
}

fn export(repo: &Path) -> Value {
    serde_json::from_str(&Engine::open(repo).unwrap().export().unwrap()).unwrap()
}

fn rows<'a>(export: &'a Value, section: &str, key: &str, caller: &str) -> Vec<&'a Value> {
    export[section]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| {
            r[key]
                .as_str()
                .is_some_and(|id| id.split("::").nth(2) == Some(caller))
        })
        .collect()
}

fn one_link(json: &Value, caller: &str, via: &str, contract: &str, level: &str) {
    let consumers = rows(json, "consumers", "site", caller);
    assert_eq!(consumers.len(), 1, "{consumers:?}");
    assert_eq!(consumers[0]["via"], via, "{consumers:?}");
    let links = rows(json, "links", "consumer", caller);
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0]["contract"], contract);
    assert_eq!(links[0]["level"], level);
    assert!(
        links[0]["provider"]
            .as_str()
            .unwrap()
            .starts_with("example.com/acme/peer::")
    );
}

const STRIP: &str = r"base.replace(/\/$/, '')";
const CONFIG: &str = "config.getString('a.url')";
const HOST: &str = "host:orders.example.com";
const ORDERS: &str = "http::GET::/orders";

#[test]
fn a_trailing_slash_strip_reads_its_receiver() {
    for (i, form) in [
        STRIP,
        r#"base.replace(/\/+$/, "")"#,
        r"base.replace(/\/*$/, ``)",
        r"base.replace(/\/$/g, '')",
        r"base.replace(/\/+$/g, '')",
        r"base.replace(/\/*$/g, '')",
    ]
    .iter()
    .enumerate()
    {
        let repo = fixture(&format!("http_base_strip_{i}"));
        replace(&repo, "src/api.ts", STRIP, form);
        one_link(&export(&repo), "Api.list", HOST, ORDERS, "exact");
    }
}

#[test]
fn an_included_variable_is_the_base() {
    let repo = fixture("http_base_include");
    replace(
        &repo,
        "partials/urls.yaml",
        "url: https://orders.example.com\n",
        "url: ${A_URL}\n",
    );
    one_link(&export(&repo), "Api.list", "env:A_URL", ORDERS, "inferred");
}

#[test]
fn a_config_chain_prefixes_the_key() {
    for (i, read) in [
        "config.getConfig('a').getString('url')",
        "config.getOptionalConfig('a').getOptionalString('url')",
        "config.getConfig('a').getOptionalConfig('b').getString('url')",
    ]
    .iter()
    .enumerate()
    {
        let repo = fixture(&format!("http_base_chain_{i}"));
        replace(&repo, "src/api.ts", CONFIG, read);
        let contract = if i == 2 {
            "http::GET::/v2/orders"
        } else {
            ORDERS
        };
        one_link(&export(&repo), "Api.list", HOST, contract, "exact");
    }
}

#[test]
fn a_factory_built_client_reads_the_base() {
    let repo = fixture("http_base_factory");
    replace(
        &repo,
        "src/api.ts",
        "export const api = new Api(config.getString('a.url'));",
        "function make(config) { return new Api(config.getString('a.url')); }\nexport const api = make(config);",
    );
    one_link(&export(&repo), "Api.list", HOST, ORDERS, "exact");
}

#[test]
fn two_literals_give_two_inferred_links() {
    let repo = fixture("http_base_several");
    std::fs::write(
        repo.join("app-config.local.yaml"),
        "a:\n  url: https://orders.example.com/v2\n",
    )
    .unwrap();
    let json = export(&repo);
    let mut links: Vec<_> = rows(&json, "links", "consumer", "Api.list")
        .into_iter()
        .map(|r| {
            assert!(
                r["provider"]
                    .as_str()
                    .unwrap()
                    .starts_with("example.com/acme/peer::")
            );
            (
                r["contract"].as_str().unwrap(),
                r["level"].as_str().unwrap(),
            )
        })
        .collect();
    links.sort_unstable();
    assert_eq!(
        links,
        [(ORDERS, "inferred"), ("http::GET::/v2/orders", "inferred")]
    );
}

#[test]
fn other_replaces_read_nothing() {
    for (i, form) in [
        r"base.replace('a', 'b')",
        r"base.replace(/x/g, '')",
        r"base.replace(/\/$/, '-')",
        r"base.replace('/', '')",
        r"base.replace(/\/$/m, '')",
        r"base.replace(/\//g, '')",
        r"base.replace(/\/$/, '/')",
    ]
    .iter()
    .enumerate()
    {
        let repo = fixture(&format!("http_base_other_{i}"));
        replace(&repo, "src/api.ts", STRIP, form);
        let json = export(&repo);
        let consumers = rows(&json, "consumers", "site", "Api.list");
        assert_eq!(consumers.len(), 1, "{consumers:?}");
        assert!(
            consumers[0]["via"].is_null() && consumers[0]["contract"].is_null(),
            "{form}: {consumers:?}"
        );
        assert!(rows(&json, "links", "consumer", "Api.list").is_empty());
    }
}

#[test]
fn a_chained_config_read_names_the_topic() {
    let json = export(&fixture("http_base_topic"));
    let producers = rows(&json, "providers", "site", "publish");
    assert_eq!(producers.len(), 1, "{producers:?}");
    assert_eq!(
        producers[0]["contract"], "channel::kafka::orders.created",
        "{producers:?}"
    );
}

#[test]
fn a_strip_removes_the_actual_trailing_slash_before_the_path() {
    let repo = fixture("http_base_strip_value");
    replace(
        &repo,
        "partials/urls.yaml",
        "url: https://orders.example.com\n",
        "url: https://orders.example.com/v2/\n",
    );
    one_link(
        &export(&repo),
        "Api.list",
        HOST,
        "http::GET::/v2/orders",
        "exact",
    );
}

#[test]
fn a_single_slash_and_a_run_of_slashes_are_different_operations() {
    for (i, (source, path)) in [
        (STRIP, "/v2//orders"),
        (r"base.replace(/\/$/g, '')", "/v2//orders"),
        (r"base.replace(/\/+$/, '')", "/v2/orders"),
        (r"base.replace(/\/*$/g, '')", "/v2/orders"),
    ]
    .into_iter()
    .enumerate()
    {
        let repo = fixture(&format!("http_slash_count_{i}"));
        replace(
            &repo,
            "src/api.ts",
            CONFIG,
            "'https://orders.example.com/v2//'",
        );
        replace(&repo, "src/api.ts", STRIP, source);
        one_link(
            &export(&repo),
            "Api.list",
            HOST,
            &format!("http::GET::{path}"),
            "exact",
        );
    }
}

#[test]
fn plain_concatenation_preserves_an_internal_empty_segment() {
    let repo = fixture("http_base_raw");
    replace(&repo, "src/api.ts", STRIP, "base");
    replace(
        &repo,
        "src/api.ts",
        CONFIG,
        "'https://orders.example.com/v2/'",
    );
    one_link(
        &export(&repo),
        "Api.list",
        HOST,
        "http::GET::/v2//orders",
        "exact",
    );
}

#[test]
fn a_strip_waits_for_the_environment_value_from_the_workload() {
    for (i, (strip, path)) in [
        (STRIP, "/v2//orders"),
        (r"base.replace(/\/+$/, '')", "/v2/orders"),
    ]
    .into_iter()
    .enumerate()
    {
        let repo = fixture(&format!("http_base_workload_{i}"));
        replace(&repo, "src/api.ts", STRIP, strip);
        replace(
            &repo,
            "partials/urls.yaml",
            "url: https://orders.example.com\n",
            "url: ${A_URL}\n",
        );
        std::fs::write(repo.join("compose.yaml"), "services:\n  client:\n    build: ./src\n    environment:\n      A_URL: https://orders.example.com/v2//\n").unwrap();
        one_link(
            &export(&repo),
            "Api.list",
            "env:A_URL",
            &format!("http::GET::{path}"),
            "inferred",
        );
    }
}

#[test]
fn a_strip_inside_a_template_only_changes_its_receiver() {
    let repo = fixture("http_base_template");
    replace(&repo, "src/api.ts", STRIP, "base");
    replace(
        &repo,
        "src/api.ts",
        CONFIG,
        "'https://orders.example.com/v2/'",
    );
    replace(
        &repo,
        "src/api.ts",
        "${this.base}/orders",
        r"${this.base}${''.replace(/\/+$/, '')}/orders",
    );
    one_link(
        &export(&repo),
        "Api.list",
        HOST,
        "http::GET::/v2//orders",
        "exact",
    );
    replace(
        &repo,
        "src/api.ts",
        r"${this.base}${''.replace(/\/+$/, '')}/orders",
        r"${(this.base + '/').replace(/\/+$/, '')}/orders",
    );
    one_link(
        &export(&repo),
        "Api.list",
        HOST,
        "http::GET::/v2/orders",
        "exact",
    );
}

#[test]
fn a_late_base_uses_the_suffix_as_written() {
    let repo = fixture("http_base_late_suffix");
    replace(&repo, "src/api.ts", "}/orders", "}orders");
    replace(
        &repo,
        "partials/urls.yaml",
        "url: https://orders.example.com\n",
        "url: ${A_URL}\n",
    );
    std::fs::write(repo.join("compose.yaml"), "services:\n  client:\n    build: ./src\n    environment:\n      A_URL: https://orders.example.com/v2//\n").unwrap();
    one_link(
        &export(&repo),
        "Api.list",
        "env:A_URL",
        "http::GET::/v2/orders",
        "inferred",
    );
}

const DISCOVERY: &str = "return this.discovery.getBaseUrl('orders');";
const INDEXED: &str = "this.perEnv[env].url";

fn no_base(repo: &Path, caller: &str) {
    let json = export(repo);
    let consumers = rows(&json, "consumers", "site", caller);
    assert_eq!(consumers.len(), 1, "{consumers:?}");
    assert!(consumers[0]["contract"].is_null(), "{consumers:?}");
    assert!(rows(&json, "links", "consumer", caller).is_empty());
}

#[test]
fn a_method_returning_a_discovery_base_is_the_base() {
    let repo = fixture("http_base_method");
    one_link(
        &export(&repo),
        "Client.list",
        "client:Backstage plugin orders at /api/orders",
        "http::GET::/api/orders/list",
        "inferred",
    );
}

#[test]
fn a_method_with_two_returns_reads_nothing() {
    let repo = fixture("http_base_two_returns");
    replace(
        &repo,
        "src/method.ts",
        DISCOVERY,
        "if (flag) return this.discovery.getBaseUrl('orders'); return 'https://orders.example.com';",
    );
    no_base(&repo, "Client.list");
}

#[test]
fn a_method_reading_a_parameter_reads_nothing() {
    let repo = fixture("http_base_method_param");
    replace(
        &repo,
        "src/method.ts",
        "async base()",
        "async base(id = 'orders')",
    );
    replace(
        &repo,
        "src/method.ts",
        DISCOVERY,
        "return this.discovery.getBaseUrl(id);",
    );
    no_base(&repo, "Client.list");
}

#[test]
fn a_record_indexed_by_a_parameter_gives_one_inferred_link_per_value() {
    let repo = fixture("http_base_record");
    inferred_record_links(&repo);
}

fn inferred_record_links(repo: &Path) {
    let json = export(repo);
    let mut links: Vec<_> = rows(&json, "links", "consumer", "Store.list")
        .into_iter()
        .map(|r| {
            (
                r["contract"].as_str().unwrap(),
                r["level"].as_str().unwrap(),
            )
        })
        .collect();
    links.sort_unstable();
    assert_eq!(
        links,
        [(ORDERS, "inferred"), ("http::GET::/v2/orders", "inferred")]
    );
}

#[test]
fn a_record_indexed_by_a_literal_gives_that_value() {
    let repo = fixture("http_base_one_entry");
    replace(&repo, "src/record.ts", INDEXED, "this.perEnv['b'].url");
    one_link(
        &export(&repo),
        "Store.list",
        HOST,
        "http::GET::/v2/orders",
        "exact",
    );
}

#[test]
fn a_record_written_outside_the_constructor_reads_nothing() {
    let repo = fixture("http_base_record_write");
    replace(
        &repo,
        "src/record.ts",
        "  list(env:",
        "  reset(next) { this.perEnv = next; }\n  list(env:",
    );
    no_base(&repo, "Store.list");
}

#[test]
fn a_key_outside_the_record_reads_nothing() {
    let repo = fixture("http_base_unlisted");
    replace(&repo, "src/record.ts", INDEXED, "this.perEnv['c'].url");
    no_base(&repo, "Store.list");
}

#[test]
fn a_spread_in_the_record_reads_nothing() {
    let repo = fixture("http_base_spread");
    replace(
        &repo,
        "src/record.ts",
        "this.perEnv = {",
        "this.perEnv = { ...config,",
    );
    no_base(&repo, "Store.list");
}

#[test]
fn writing_an_entry_invalidates_the_record() {
    for (i, target) in ["this.perEnv.a.url", "this.perEnv['a'].url", "this.perEnv.a"]
        .into_iter()
        .enumerate()
    {
        let repo = fixture(&format!("http_base_record_entry_write_{i}"));
        replace(&repo, "src/record.ts", INDEXED, "this.perEnv['a'].url");
        replace(
            &repo,
            "src/record.ts",
            "  list(env:",
            &format!("  reset(next) {{ {target} = next; }}\n  list(env:"),
        );
        no_base(&repo, "Store.list");
    }
}

#[test]
fn a_record_key_uses_the_constant_in_scope_at_the_read() {
    let repo = fixture("http_base_record_key_scope");
    replace(
        &repo,
        "src/record.ts",
        "declare const config: any;",
        "declare const config: any;\nconst env = 'a';",
    );
    replace(
        &repo,
        "src/record.ts",
        "list(env: string) {",
        "list() { { const env = 'b'; void env; }",
    );
    one_link(&export(&repo), "Store.list", HOST, ORDERS, "exact");
}

#[test]
fn a_record_key_alias_uses_its_own_declaration_site() {
    let repo = fixture("http_base_record_key_alias");
    replace(
        &repo,
        "src/record.ts",
        "list(env: string) {",
        "list() { { const env = 'b'; void env; } const env = 'a'; const key = env; { const env = 'b';",
    );
    replace(&repo, "src/record.ts", INDEXED, "this.perEnv[key].url");
    replace(&repo, "src/record.ts", "/orders`); }", "/orders`); } }");
    one_link(&export(&repo), "Store.list", HOST, ORDERS, "exact");
}

#[test]
fn a_mutable_record_key_hides_a_constant() {
    let repo = fixture("http_base_record_key_mutable");
    replace(
        &repo,
        "src/record.ts",
        "list(env: string) {",
        "list(input: string) { { const env = 'a'; void env; } let env = input;",
    );
    inferred_record_links(&repo);
}

#[test]
fn discovery_requests_keep_the_direct_and_same_class_sites_when_callers_become_exact() {
    let json = export(&fixture("http_discovery_sites"));
    let via = "client:Backstage plugin orders at /api/orders";
    for (caller, method) in [
        ("Orders.get", "GET"),
        ("readOrder", "GET"),
        ("Orders.fail", "PUT"),
        ("updateOrder", "PUT"),
    ] {
        one_link(
            &json,
            caller,
            via,
            &format!("http::{method}::/api/orders/list/{{}}"),
            "inferred",
        );
    }
}

#[test]
fn a_bare_environment_endpoint_does_not_imply_a_root_route() {
    let repo = fixture("http_bare_env");
    let json = export(&repo);
    let bare = rows(&json, "consumers", "site", "Sender.postBase");
    assert_eq!(bare.len(), 1);
    assert_eq!(bare[0]["via"], "env:ENDPOINT_URL");
    assert!(bare[0]["contract"].is_null(), "{bare:?}");
    assert!(rows(&json, "links", "consumer", "Sender.postBase").is_empty());
    one_link(
        &json,
        "postRoot",
        "env:ENDPOINT_URL",
        "http::POST::/",
        "inferred",
    );
    std::fs::write(repo.join("compose.yaml"), "services:\n  client:\n    build: ./src\n    environment:\n      ENDPOINT_URL: https://orders.example.com/orders\n").unwrap();
    one_link(
        &export(&repo),
        "Sender.postBase",
        "env:ENDPOINT_URL",
        "http::POST::/orders",
        "inferred",
    );
}
