use crate::common::{edited, fixture_repo, in_repo};
use meada::engine::Engine;
use serde_json::Value;
use std::path::{Path, PathBuf};

const SERVICE: &str = "plugins/orders/src/service.ts";
const BUILD: &str = "plugins/orders/src/build.ts";
const CONSUMER: &str = "plugins/approvals/src/consumer.ts";
const REPO: &str = "example.com/acme/field-flow";
const APPROVED: &str = "channel::kafka::orders.approved";

fn repo(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "ts-field-flow",
        Some("https://example.com/acme/field-flow.git"),
    )
}

fn replace(repo: &Path, path: &str, from: &str, to: &str) {
    let path = repo.join(path);
    let src = std::fs::read_to_string(&path).unwrap();
    assert!(src.contains(from), "{path:?}: {from}");
    std::fs::write(path, src.replace(from, to)).unwrap();
}

fn export(repo: &Path) -> Value {
    serde_json::from_str(&Engine::open(repo).unwrap().export().unwrap()).unwrap()
}

fn topics(export: &Value) -> Vec<&str> {
    let mut topics: Vec<_> = export["providers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| {
            p["contract"]
                .as_str()
                .filter(|c| c.starts_with("channel::"))
        })
        .collect();
    topics.sort_unstable();
    topics
}

fn assigned(repo: &Path, visibility: &str) {
    replace(
        repo,
        SERVICE,
        "constructor(private client: KafkaClient, private topic: string) {}",
        &format!(
            "{visibility} topic: string;\n  constructor(private client: KafkaClient, topic: string) {{ this.topic = topic; }}"
        ),
    );
}

#[test]
fn a_topic_held_from_the_constructor_reaches_the_side() {
    for (case, explicit) in [("field_property", false), ("field_assignment", true)] {
        let repo = repo(case);
        if explicit {
            assigned(&repo, "private");
        }
        let export = export(&repo);
        assert_eq!(topics(&export), [APPROVED]);
        let links: Vec<_> = export["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| {
                l["contract"] == APPROVED && l["consumer"].as_str().unwrap().contains(CONSUMER)
            })
            .collect();
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0]["level"], "exact");
    }
}

#[test]
fn two_constructions_give_both_topics() {
    let repo = repo("field_two_constructions");
    replace(
        &repo,
        BUILD,
        "return make(",
        "new OrderService(new KafkaClient(), config.getString('acme.orders.audit'));\n  new OrderService(new KafkaClient(), config.getString('acme.orders.topic'));\n  return make(",
    );
    let export = export(&repo);
    assert_eq!(topics(&export), [APPROVED, "channel::kafka::orders.audit"]);
}

#[test]
fn a_public_field_built_in_a_factory_gives_the_topic() {
    let repo = repo("field_public");
    assigned(&repo, "public");
    assert_eq!(topics(&export(&repo)), [APPROVED]);
}

#[test]
fn a_test_construction_adds_no_production_topic() {
    let repo = repo("field_test_construction");
    assert_eq!(topics(&export(&repo)), [APPROVED]);
}

#[test]
fn a_field_written_again_keeps_the_topic_unknown() {
    for (case, extra) in [
        (
            "field_write_same",
            "reset() { this.topic = 'orders.audit'; }",
        ),
        (
            "field_write_subclass",
            "} export class Sub extends OrderService { reset() { this.topic = 'orders.audit'; }",
        ),
        (
            "field_write_unknown",
            "} export function reset(other: any) { other.topic = 'orders.audit';",
        ),
    ] {
        let repo = repo(case);
        assigned(&repo, "public");
        replace(&repo, SERVICE, "\n}\n", &format!("\n  {extra}\n}}\n"));
        assert_eq!(topics(&export(&repo)), ["channel::kafka"], "{case}");
    }
}

#[test]
fn an_heir_keeps_the_topic_unknown() {
    let repo = repo("field_heir");
    std::fs::write(repo.join("plugins/approvals/src/sub.ts"), "import { OrderService } from '../../orders/src/service'; export class Sub extends OrderService {}").unwrap();
    assert_eq!(topics(&export(&repo)), ["channel::kafka"]);
}

#[test]
fn a_construction_that_is_not_exact_keeps_the_topic_unknown() {
    let repo = repo("field_unknown_construction");
    std::fs::write(repo.join(BUILD), "import { KafkaClient } from './client';\nexport function make(OrderService: any) { return new OrderService(new KafkaClient(), 'orders.audit'); }").unwrap();
    let export = export(&repo);
    let call = export["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["id"]
                .as_str()
                .is_some_and(|id| id.contains("build.ts::make::OrderService#"))
        })
        .unwrap();
    assert_eq!(call["level"], "inferred", "{call}");
    assert!(
        call["target"]
            .as_str()
            .unwrap()
            .ends_with("::plugins/orders/src/service.ts::OrderService")
    );
    assert_eq!(topics(&export), ["channel::kafka"]);
}

#[test]
fn java_and_kotlin_constructor_fields_keep_todays_answer() {
    let repo = repo("field_other_languages");
    std::fs::write(repo.join("JavaApi.java"), "import org.springframework.web.client.RestTemplate;\nclass JavaApi { private final RestTemplate rest = new RestTemplate(); private final String base; JavaApi(String base) { this.base = base; } void list() { rest.getForObject(base + \"/orders\", String.class); } static JavaApi make() { return new JavaApi(\"https://orders.example.com\"); }}").unwrap();
    std::fs::write(repo.join("KotlinApi.kt"), "import org.springframework.web.client.RestTemplate\nclass KotlinApi(private val base: String, private val rest: RestTemplate) { fun list() { rest.getForObject(\"$base/orders\", String::class.java) } }\nfun make() = KotlinApi(\"https://orders.example.com\", RestTemplate())").unwrap();
    let export = export(&repo);
    for site in ["JavaApi.java::JavaApi.list", "KotlinApi.kt::KotlinApi.list"] {
        let rows: Vec<_> = export["consumers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["site"].as_str().unwrap().contains(site))
            .collect();
        assert_eq!(rows.len(), 1, "{site}: {rows:?}");
        assert!(rows[0]["contract"].is_null(), "{site}: {rows:?}");
        assert!(rows[0]["via"].is_null(), "{site}: {rows:?}");
    }
}

#[test]
fn a_base_held_from_the_constructor_reaches_the_route() {
    let case = "field_http";
    fixture_repo(
        case,
        "http-iac-server",
        Some("https://example.com/acme/peer.git"),
    );
    let repo = repo(case);
    let manifest = repo.join("meada.toml");
    let config = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        manifest,
        format!("{config}\n[[peer]]\nrepo = \"example.com/acme/peer\"\n"),
    )
    .unwrap();
    let export = export(&repo);
    let row = export["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["site"].as_str().unwrap().contains("api.ts::Api.list"))
        .unwrap();
    assert_eq!(row["via"], "host:orders.example.com", "{row}");
    assert_eq!(row["contract"], "http::GET::/orders", "{row}");
    assert!(export["links"].as_array().unwrap().iter().any(|l| {
        l["contract"] == row["contract"]
            && l["consumer"] == row["site"]
            && l["provider"]
                .as_str()
                .unwrap()
                .contains("example.com/acme/peer::app/orders/route.ts::GET")
    }));
}

#[test]
fn a_publish_change_reaches_the_consumer_at_certain() {
    let repo = repo("field_affected");
    let _ = export(&repo);
    let answer = edited(&repo, SERVICE, "\"message\"", "\"edited\"");
    let row = in_repo(&answer, REPO, CONSUMER).unwrap_or_else(|| panic!("{answer}"));
    assert_eq!(row["tier"], "certain", "{row}");
}

#[test]
fn an_unbound_message_does_not_hide_the_constructed_topic() {
    let repo = repo("field_second_argument");
    replace(
        &repo,
        SERVICE,
        "publish() { this.client.sendMessage(this.topic, \"message\")",
        "publish(message: string) { this.client.sendMessage(message, this.topic)",
    );
    replace(
        &repo,
        "meada.toml",
        "role = \"producer\"\nbroker = \"kafka\"\ntopic = \"0\"",
        "role = \"producer\"\nbroker = \"kafka\"\ntopic = \"1\"",
    );
    assert_eq!(topics(&export(&repo)), [APPROVED]);
}

#[test]
fn a_constructor_base_and_a_method_argument_reach_the_same_request() {
    let repo = repo("field_and_method");
    let path = "plugins/orders/src/api.ts";
    replace(
        &repo,
        path,
        "list() { return fetch(`${this.base}/orders`)",
        "list(route: string) { return fetch(`${this.base}/${route}`)",
    );
    replace(
        &repo,
        path,
        "new Api('https://orders.example.com');",
        "new Api('https://orders.example.com');\nexport function orders() { return api.list('orders'); }",
    );
    let export = export(&repo);
    let requests: Vec<_> = export["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c["site"]
                .as_str()
                .unwrap()
                .contains("plugins/orders/src/api.ts")
        })
        .collect();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(
        requests[0]["via"], "host:orders.example.com",
        "{requests:?}"
    );
    assert_eq!(
        requests[0]["contract"], "http::GET::/orders",
        "{requests:?}"
    );
}

#[test]
fn a_test_caller_of_a_factory_adds_no_production_topic() {
    let repo = repo("field_test_factory");
    replace(&repo, BUILD, "function make(", "export function make(");
    replace(
        &repo,
        "plugins/orders/src/OrderService.test.ts",
        "return new OrderService(",
        "return make(",
    );
    replace(
        &repo,
        "plugins/orders/src/OrderService.test.ts",
        "import { OrderService } from './service';",
        "import { make } from './build';",
    );
    assert_eq!(topics(&export(&repo)), [APPROVED]);
}
