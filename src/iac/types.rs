use std::sync::LazyLock;

use serde::Deserialize;

const TABLE: &str = include_str!("types.toml");

// ponytail: an ARN or a URL is well under a few hundred characters; a longer text is the name as
// it is, which keeps the matching (quadratic in the text) bounded.
const FORM_TEXT_MAX: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Contract {
    Channel,
    Resource,
}

impl Contract {
    fn prefix(self) -> &'static str {
        match self {
            Contract::Channel => "channel",
            Contract::Resource => "resource",
        }
    }
}

pub(crate) fn provisions(prefix: &str, kind: &str) -> bool {
    ENTRIES
        .iter()
        .any(|e| e.kind == kind && e.contract.prefix() == prefix)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub format: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub contract: Contract,
    pub kind: String,
    pub name: String,
    pub fallback: Option<String>,
    pub basis: Option<String>,
    pub qualifier: Option<String>,
    pub qualifier_ref: Option<String>,
    pub qualifier_type: Option<String>,
    pub source: Option<String>,
    pub source_ref: Option<String>,
    pub source_type: Option<String>,
    pub filter: Option<String>,
    #[serde(default)]
    pub forms: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    entry: Vec<Entry>,
    #[serde(default)]
    workload: Vec<Workload>,
    #[serde(default)]
    module: Vec<Registry>,
    #[serde(default)]
    topology: Vec<Topology>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TopologyRole {
    Queue,
    Exchange,
    Binding,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Topology {
    pub format: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub role: TopologyRole,
}

static TOPOLOGY: LazyLock<Vec<Topology>> =
    LazyLock::new(|| toml::from_str::<Table>(TABLE).map_or_else(|_| Vec::new(), |t| t.topology));

pub(crate) fn topology(ty: &str) -> Option<TopologyRole> {
    TOPOLOGY
        .iter()
        .find(|row| row.format == "kubernetes" && row.ty == ty)
        .map(|row| row.role)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Registry {
    pub source: String,
    pub entry: String,
    pub input: String,
    pub create: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
    pub fifo: Option<String>,
    pub prefix: Option<String>,
}

static REGISTRY: LazyLock<Vec<Registry>> =
    LazyLock::new(|| toml::from_str::<Table>(TABLE).map_or_else(|_| Vec::new(), |t| t.module));

pub(crate) fn registry(source: &str) -> Option<&'static Registry> {
    let source = source
        .strip_prefix("registry.terraform.io/")
        .unwrap_or(source);
    REGISTRY.iter().find(|m| m.source == source)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Workload {
    pub format: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub images: Vec<String>,
    pub code: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub code_file: bool,
}

static WORKLOADS: LazyLock<Vec<Workload>> =
    LazyLock::new(|| toml::from_str::<Table>(TABLE).map_or_else(|_| Vec::new(), |t| t.workload));

pub(crate) fn workload(format: &str, ty: &str) -> Option<&'static Workload> {
    WORKLOADS.iter().find(|w| w.format == format && w.ty == ty)
}

pub(crate) fn named_workload(id: &str) -> Option<&'static Workload> {
    let (format, ty) = id.split_once(':')?;
    workload(format, ty)
}

pub(crate) fn workload_of(node: &super::Node) -> Option<&'static Workload> {
    if let Some(id) = node.provisioned.as_ref().and_then(|p| p.entry.as_deref()) {
        return named_workload(id);
    }
    workload(format_of(&node.path)?, &node.ty)
}

pub(super) static ENTRIES: LazyLock<Vec<Entry>> =
    LazyLock::new(|| toml::from_str::<Table>(TABLE).map_or_else(|_| Vec::new(), |t| t.entry));

pub(crate) fn entry(format: &str, ty: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.format == format && e.ty == ty)
}

pub(crate) fn named(id: &str) -> Option<&'static Entry> {
    let (format, ty) = id.split_once(':')?;
    entry(format, ty)
}

pub(crate) fn node_entry(node: &super::Node) -> Option<&'static Entry> {
    match node.provisioned.as_ref().and_then(|p| p.entry.as_deref()) {
        Some(id) => named(id),
        None => provision_entry(&node.path, &node.block, &node.ty),
    }
}

pub(crate) fn entries(format: &str) -> impl Iterator<Item = &'static Entry> + '_ {
    ENTRIES.iter().filter(move |e| e.format == format)
}

pub(crate) fn stamp() -> blake3::Hash {
    blake3::hash(TABLE.as_bytes())
}

pub(crate) fn provision_entry(path: &str, block: &str, ty: &str) -> Option<&'static Entry> {
    let format = format_of(path)?;
    let declared = match format {
        "terraform" => super::is_resource_block(block),
        "cloudformation" => {
            block.starts_with("Resources.") || block.starts_with("resources.Resources.")
        }
        _ => false,
    };
    declared.then(|| entry(format, ty)).flatten()
}

pub(crate) fn format_of(path: &str) -> Option<&'static str> {
    if path.ends_with(".tf") || path.ends_with(".tofu") || super::json::is_file(path) {
        Some("terraform")
    } else if super::cloudformation::is_source(path) {
        Some("cloudformation")
    } else {
        None
    }
}

pub(crate) fn identified(text: &str) -> Option<String> {
    if let Some(contract) = crate::contract::azure::identified(text) {
        return Some(contract);
    }
    if text.starts_with("gs://") || text.starts_with("projects/") {
        return crate::contract::gcp::identified(text);
    }
    if !(text.starts_with("arn:") || text.starts_with("https://") || text.starts_with("s3://")) {
        return None;
    }
    let service = text
        .strip_prefix("arn:")
        .and_then(|_| text.split(':').nth(2));
    let mut contracts = ENTRIES
        .iter()
        .filter(|entry| {
            service.is_none_or(|service| {
                entry
                    .forms
                    .iter()
                    .any(|form| form.starts_with("arn:") && form.split(':').nth(2) == Some(service))
            })
        })
        .filter_map(|entry| {
            let name = entry.read(text);
            (name != text).then(|| entry.contract(name))
        })
        .collect::<std::collections::BTreeSet<_>>();
    (contracts.len() == 1).then(|| contracts.pop_first().unwrap())
}

impl Entry {
    pub(crate) fn canonical(&self) -> Option<&Entry> {
        match self.basis.as_deref() {
            Some(id) => named(id).filter(|entry| entry.basis.is_none()),
            None => Some(self),
        }
    }
    pub(crate) fn mapped(&self, text: &str, pattern: bool) -> Option<String> {
        if self.basis.is_some() {
            return self.canonical()?.mapped(text, pattern);
        }
        if self.kind == "amqp" && !pattern {
            return crate::channel::amqp_name(text).then(|| text.into());
        }
        if pattern {
            if self.ty.starts_with("google_") {
                return crate::contract::gcp::pattern(&self.ty, text);
            }
            if self.ty.starts_with("azurerm_") {
                return crate::contract::azure::pattern(&self.ty, text);
            }
        }
        if text.is_empty() || text.len() > crate::config_values::VALUE_MAX {
            return None;
        }
        if self.ty == "google_sql_database" {
            let (instance, database) = text.split_once('/')?;
            return crate::contract::gcp::database(instance, database);
        }
        if self.ty == "azurerm_servicebus_subscription" {
            return crate::contract::azure::subscription(text);
        }
        let target = self.project(text)?;
        let structured = ["arn:", "projects/", "/subscriptions/", "//"]
            .iter()
            .any(|prefix| {
                target
                    .get(..prefix.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
            });
        (!structured
            && !target.contains("://")
            && !target.contains(['$', '{', '}', '?', '#'])
            && !target.chars().any(char::is_whitespace)
            && (!pattern || super::pattern::supported(&target)))
        .then_some(target)
    }

    pub(crate) fn target_is_supported(&self, name: &str) -> bool {
        if self.kind == "amqp" {
            return crate::channel::amqp_name(name);
        }
        !name.starts_with("arn:") && !name.contains("://")
    }
    pub(crate) fn defining_key(&self, block: &str) -> String {
        if self.format == "cloudformation" {
            format!("{block}.Properties.{}", self.name)
        } else {
            format!("{block}.{}", self.name)
        }
    }

    pub(crate) fn project(&self, text: &str) -> Option<String> {
        if self.basis.is_some() {
            return self.canonical()?.project(text);
        }
        use crate::contract::gcp;
        match self.ty.as_str() {
            "azurerm_servicebus_queue" | "azurerm_servicebus_topic" => {
                crate::contract::azure::servicebus(text)
            }
            "azurerm_servicebus_subscription" => crate::contract::azure::subscription(text)
                .or_else(|| crate::contract::azure::servicebus(text).filter(|s| !s.contains('/'))),
            "azurerm_eventhub" => crate::contract::azure::eventhub(text),
            "azurerm_storage_account" => crate::contract::azure::account(text),
            "azurerm_storage_container" => crate::contract::azure::container(text),
            "azurerm_key_vault_secret" => crate::contract::azure::secret(text),
            "google_pubsub_topic" => gcp::pubsub(text, false),
            "google_pubsub_subscription" => gcp::pubsub(text, true),
            "google_storage_bucket" => gcp::bucket(text),
            "google_sql_database_instance" => gcp::instance(text),
            "google_sql_database" => gcp::database_name(text).then(|| text.into()),
            "google_secret_manager_secret" => gcp::secret(text),
            _ => Some(self.read(text).into()),
        }
    }
    pub(crate) fn contract(&self, name: &str) -> String {
        format!("{}::{}::{name}", self.contract.prefix(), self.kind)
    }

    pub(crate) fn read<'t>(&self, text: &'t str) -> &'t str {
        if text.len() > FORM_TEXT_MAX {
            return text;
        }
        (self.forms.iter())
            .find_map(|form| fits(form, text))
            .unwrap_or(text)
    }
}

#[derive(Clone, Copy)]
enum Piece<'f> {
    Text(&'f str),
    Any,
    Name(bool),
    Suffix,
}

fn pieces(form: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = form;
    while !rest.is_empty() {
        let star = rest.find('*');
        let name = rest.find("{name}").or_else(|| rest.find("{path}"));
        let suffix = rest.find('?');
        let next = [star, name, suffix].into_iter().flatten().min();
        match next {
            Some(0) if star == Some(0) => {
                out.push(Piece::Any);
                rest = &rest[1..];
            }
            Some(0) if name == Some(0) => {
                out.push(Piece::Name(rest.starts_with("{path}")));
                rest = &rest["{name}".len()..];
            }
            Some(0) => {
                out.push(Piece::Suffix);
                rest = &rest[1..];
            }
            Some(at) => {
                out.push(Piece::Text(&rest[..at]));
                rest = &rest[at..];
            }
            None => {
                out.push(Piece::Text(rest));
                rest = "";
            }
        }
    }
    out
}

fn fits<'t>(form: &str, text: &'t str) -> Option<&'t str> {
    let pieces = pieces(form);
    let mut failed = std::collections::HashSet::new();
    let mut name = None;
    match matches(&pieces, text, 0, 0, &mut failed, &mut name) {
        true => name,
        false => None,
    }
}

fn matches<'t>(
    pieces: &[Piece],
    text: &'t str,
    p: usize,
    at: usize,
    failed: &mut std::collections::HashSet<(usize, usize)>,
    name: &mut Option<&'t str>,
) -> bool {
    if failed.contains(&(p, at)) {
        return false;
    }
    let found = match pieces.get(p) {
        None => {
            at == text.len()
                || (!pieces.iter().any(|p| matches!(p, Piece::Name(true)))
                    && text[at..].starts_with('/'))
        }
        Some(Piece::Text(t)) => {
            text[at..].starts_with(t) && matches(pieces, text, p + 1, at + t.len(), failed, name)
        }
        Some(Piece::Suffix) => {
            text.as_bytes()
                .get(at)
                .is_some_and(u8::is_ascii_alphanumeric)
                && matches(pieces, text, p + 1, at + 1, failed, name)
        }
        Some(piece @ (Piece::Any | Piece::Name(_))) => {
            let run = text[at..]
                .find(|c| c == ':' || (c == '/' && !matches!(piece, Piece::Name(true))))
                .map_or(text.len(), |i| at + i);
            let ends = (at + 1..=run).filter(|&e| text.is_char_boundary(e));
            let mut hit = false;
            for end in ends {
                if matches(pieces, text, p + 1, end, failed, name) {
                    if matches!(piece, Piece::Name(_)) {
                        *name = Some(&text[at..end]);
                    }
                    hit = true;
                    break;
                }
            }
            hit
        }
    };
    if !found {
        failed.insert((p, at));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_parses() {
        let table: Table = toml::from_str(TABLE).unwrap();
        assert!(!table.entry.is_empty());
        assert_eq!(table.entry.len(), ENTRIES.len());
    }

    #[test]
    fn stored_projection_defaults_preserve_the_explicit_unknown_state() {
        let old: crate::iac::Projection =
            serde_json::from_str(r#"{"name":null,"pattern":false,"through":[]}"#).unwrap();
        assert!(old.name.is_none() && old.entry.is_none() && old.binding.is_none());
        assert!(old.defining.is_empty() && old.code.is_none());
        let known: crate::iac::Projection =
            serde_json::from_str(r#"{"name":"literal","pattern":false,"through":[]}"#).unwrap();
        assert_eq!(known.name.as_deref(), Some("literal"));
        assert_eq!(
            serde_json::to_value(known).unwrap(),
            serde_json::json!({"name":"literal","pattern":false,"through":[]})
        );
    }

    #[test]
    fn every_entry_has_a_fixture_case() {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/provision-infra"
        );
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut files = Vec::new();
        let mut dirs = vec![
            std::path::PathBuf::from(root),
            std::path::PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/aws-infra"
            )),
            fixtures.join("aws-workload-aliases"),
            fixtures.join("azure-infra"),
            fixtures.join("gcp-infra"),
            fixtures.join("custom-strimzi"),
            fixtures.join("custom-inventory"),
        ];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let path = e.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if format_of(&path.to_string_lossy()).is_some() {
                    files.push((
                        path.strip_prefix(&fixtures)
                            .unwrap()
                            .to_string_lossy()
                            .to_string(),
                        std::fs::read(&path).unwrap(),
                    ));
                }
            }
        }
        let (mut nodes, _) = crate::iac::evaluate(&files);
        nodes.extend(crate::kube::evaluate(&files).nodes);
        for e in ENTRIES.iter() {
            assert!(
                nodes.iter().any(|n| node_entry(n)
                    .is_some_and(|known| known.format == e.format && known.ty == e.ty)
                    && n.provisioned
                        .as_ref()
                        .map_or(n.name.as_deref(), |p| p.name.as_deref())
                        .is_some_and(|name| !name.is_empty())),
                "no evaluated fixture case for {} {}",
                e.format,
                e.ty
            );
        }
    }

    #[test]
    fn every_registry_source_has_a_named_evaluated_fixture_case() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/custom-registry/main.tf");
        let files = vec![("main.tf".into(), std::fs::read(path).unwrap())];
        let repository = crate::iac::Repository::read(&files);
        let (nodes, _) = repository.evaluate(|_| true, super::super::STORED_MAX);
        for row in REGISTRY.iter() {
            let blocks = repository
                .modules
                .values()
                .flat_map(|m| &m.blocks)
                .filter(|b| b.source.as_deref() == Some(row.source.as_str()));
            assert!(
                blocks.into_iter().any(|b| nodes.iter().any(|n| {
                    n.address == format!("module.{}", b.name)
                        && n.provisioned.as_ref().is_some_and(|p| {
                            p.entry.as_deref() == Some(row.entry.as_str())
                                && (p.name.is_some() || !n.images.is_empty())
                        })
                })),
                "no evaluated registry fixture for {}",
                row.source
            );
        }
    }

    #[test]
    fn every_topology_role_has_a_named_or_routed_evaluated_fixture_case() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/custom-rabbit/resources.yaml");
        let out = crate::kube::evaluate(&[("resources.yaml".into(), std::fs::read(path).unwrap())]);
        for row in TOPOLOGY.iter() {
            assert!(
                out.nodes.iter().any(|node| {
                    node.address.starts_with(&format!("{}/", row.ty))
                        && match row.role {
                            TopologyRole::Binding => node
                                .provisioned
                                .as_ref()
                                .is_some_and(|p| p.binding.is_some()),
                            _ => node.name.as_deref().is_some_and(|name| !name.is_empty()),
                        }
                }),
                "no evaluated topology fixture for {}",
                row.ty
            );
        }
    }

    #[test]
    fn the_entries_name_their_contract_and_argument() {
        let sqs = entry("terraform", "aws_sqs_queue").unwrap();
        assert_eq!(sqs.contract("orders"), "channel::sqs::orders");
        assert_eq!(sqs.name, "name");
        let s3 = entry("terraform", "aws_s3_bucket").unwrap();
        assert_eq!(s3.contract("archive"), "resource::s3::archive");
        assert_eq!(s3.name, "bucket");
        assert!(entry("terraform", "aws_unknown_resource").is_none());
        assert!(entry("cloudformation", "aws_sqs_queue").is_none());
    }

    #[test]
    fn a_name_is_read_out_of_an_arn_or_a_url() {
        let sqs = entry("terraform", "aws_sqs_queue").unwrap();
        assert_eq!(
            sqs.read("arn:aws:sqs:eu-west-1:123456789012:orders"),
            "orders"
        );
        let url = "https://sqs.eu-west-1.amazonaws.com/123456789012/orders.fifo";
        assert_eq!(sqs.read(url), "orders.fifo");
        let legacy = "https://eu-west-1.queue.amazonaws.com/123456789012/refunds";
        assert_eq!(sqs.read(legacy), "refunds");
        assert_eq!(sqs.read("orders"), "orders");
        assert_eq!(
            sqs.read("arn:aws:sqs:eu-west-1::orders"),
            "arn:aws:sqs:eu-west-1::orders"
        );
        let s3 = entry("terraform", "aws_s3_bucket").unwrap();
        assert_eq!(s3.read("arn:aws:s3:::orders-archive"), "orders-archive");
        assert_eq!(
            s3.read("s3://orders-archive/2026/11/x.json"),
            "orders-archive"
        );
        assert_eq!(
            s3.read("https://orders.archive.s3.amazonaws.com"),
            "orders.archive"
        );
        let regional = "https://orders-archive.s3.eu-west-1.amazonaws.com/key";
        assert_eq!(s3.read(regional), "orders-archive");
        let path = "https://s3.eu-west-1.amazonaws.com/orders-archive/key";
        assert_eq!(s3.read(path), "orders-archive");
        assert_eq!(s3.read("s3://a:b"), "s3://a:b");
    }

    #[test]
    fn a_text_past_the_form_ceiling_is_the_name_as_it_is() {
        let sqs = entry("terraform", "aws_sqs_queue").unwrap();
        let long = format!("arn:aws:sqs:eu-west-1:1:{}", "q".repeat(FORM_TEXT_MAX));
        assert_eq!(sqs.read(&long), long);
        let short = format!("arn:aws:sqs:eu-west-1:1:{}", "q".repeat(100));
        assert_eq!(sqs.read(&short), "q".repeat(100));
    }

    #[test]
    fn matching_a_hostile_text_stays_bounded() {
        let s3 = entry("terraform", "aws_s3_bucket").unwrap();
        let text = format!("https://{}", ".s3".repeat(300));
        let started = std::time::Instant::now();
        let _ = s3.read(&text);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
