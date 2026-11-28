use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, Transaction, params};
use serde_json::Value;

use crate::Result;
use crate::config_values::{self, DOCUMENTS_MAX};
use crate::document::events::{self, Step, Syntax};
use crate::iac::{Node, Reference, joined};
use crate::lang::MAX_BYTES;
use crate::msg::Msg;

mod crossplane;
mod custom;
mod environment;
mod routing;
mod topology;

// ponytail: real trees nest a base or two under an overlay; 16 stops a chain built to run on.
const OVERLAY_DEPTH_MAX: usize = 16;

// ponytail: a large overlay holds a few thousand objects; 10,000 is past real ones.
const OBJECTS_MAX: usize = 10_000;

// ponytail: a large overlay copies a few MiB; sixteen times the per-file ceiling stops a large
// ConfigMap reached along thousands of paths.
const OVERLAY_BYTES_MAX: usize = 16 * MAX_BYTES;

// ponytail: a real overlay walks tens of entries; 50,000 is past any, and cheap.
const WALK_MAX: usize = 50_000;

// ponytail: a Service selects a handful of workloads; 64 is past a wide selector.
const OBJECT_REFERENCES_MAX: usize = 64;

// ponytail: four times the per-file ceiling holds a unit of a few thousand objects; past it the
// tree is built to explode.
const UNIT_STORED_MAX: usize = 4 * MAX_BYTES;

// ponytail: sixteen times the per-file ceiling holds a monorepo of overlays; many overlays over
// one exploding base would pass it.
const KUBE_STORED_MAX: usize = 16 * MAX_BYTES;

// ponytail: 128 bytes keeps a repository to about 120,000 nodes, a couple of hundred MiB, and
// leaves a unit of `OBJECTS_MAX` objects within `UNIT_STORED_MAX`.
const NODE_BYTES: usize = 128;

// ponytail: DNS limits a host to 253 bytes and Kubernetes a volume name to a 63-byte label.
const HOST_MAX: usize = 253;
const VOLUME_NAME_MAX: usize = 63;
// ponytail: a reference still reaches a longer final name through the name a stage renamed.
const OBJECT_NAME_MAX: usize = 253;

const CLUSTER_SCOPED: &[&str] = &[
    "APIService",
    "CSIDriver",
    "CSINode",
    "CertificateSigningRequest",
    "ClusterIssuer",
    "ClusterRole",
    "ClusterRoleBinding",
    "ComponentStatus",
    "CustomResourceDefinition",
    "FlowSchema",
    "IngressClass",
    "MutatingWebhookConfiguration",
    "Namespace",
    "Node",
    "PersistentVolume",
    "PodSecurityPolicy",
    "PriorityClass",
    "PriorityLevelConfiguration",
    "RuntimeClass",
    "StorageClass",
    "SubjectAccessReview",
    "TokenReview",
    "ValidatingAdmissionPolicy",
    "ValidatingAdmissionPolicyBinding",
    "ValidatingWebhookConfiguration",
    "VolumeAttachment",
];

fn cluster_scoped(kind: &str, group: &str) -> bool {
    CLUSTER_SCOPED.contains(&kind) || crossplane::cluster_scoped(group)
}

const UNPREFIXED: &[&str] = &["APIService", "CustomResourceDefinition", "Namespace"];

pub(crate) fn is_source(path: &str) -> bool {
    [".yaml", ".yml", ".json"].iter().any(|e| path.ends_with(e))
}

fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

pub(crate) fn is_kustomization(path: &str) -> bool {
    matches!(file_name(path), "kustomization.yaml" | "kustomization.yml")
}

pub(crate) fn wanted(path: &str, src: &[u8]) -> bool {
    is_source(path) && (is_kustomization(path) || src.windows(10).any(|w| w == b"apiVersion"))
}

pub(crate) fn takes(path: &str, src: &[u8]) -> bool {
    let mappings = crate::iac::mapping::Config::default();
    wanted(path, src) && (is_kustomization(path) || !read(path, src, &mappings).objects.is_empty())
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Facts {
    labels: Option<BTreeMap<String, String>>,
    selector: BTreeMap<String, String>,
    mounts: Vec<(&'static str, String, String)>,
    account: Option<(&'static str, String)>,
    backends: Vec<(String, String)>,
    ingress: Vec<crate::iac::Ingress>,
    images: Vec<(String, String)>,
    xp: crossplane::Facts,
    environment: Vec<environment::Input>,
    data: BTreeMap<String, Option<String>>,
    custom: Option<custom::Name>,
    topology: Option<topology::Facts>,
}

#[derive(Debug, Clone, PartialEq)]
struct Obj {
    group: String,
    kind: String,
    orig: String,
    name: String,
    prefixes: Vec<usize>,
    suffixes: Vec<usize>,
    moved: bool,
    ns: Option<String>,
    path: String,
    line: usize,
    end_line: usize,
    block: String,
    facts: Facts,
    weight: usize,
}

const ACCOUNT_RENAMED: [&str; 7] = [
    "Pod",
    "StatefulSet",
    "Deployment",
    "ReplicationController",
    "CronJob",
    "Job",
    "DaemonSet",
];

fn nameref_ns(ns: Option<&str>) -> Option<&str> {
    ns.filter(|&n| n != "default")
}

fn ends_alike<'s>(
    a: impl ExactSizeIterator<Item = &'s str>,
    b: impl ExactSizeIterator<Item = &'s str>,
    lax: bool,
) -> bool {
    match (a.len(), b.len()) {
        (0, n) | (n, 0) => lax || n == 0,
        _ => a.zip(b).all(|(x, y)| x == y),
    }
}

fn weight(facts: &Facts) -> usize {
    struct Count(usize);
    impl std::fmt::Write for Count {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0 += s.len();
            Ok(())
        }
    }
    let mut count = Count(0);
    let _ = std::fmt::write(&mut count, format_args!("{facts:?}"));
    count.0
}

impl Obj {
    fn outer_prefixes(&self) -> impl ExactSizeIterator<Item = &str> {
        let mut at = 0;
        (self.prefixes.iter().rev()).map(move |&n| {
            at += n;
            &self.name[at - n..at]
        })
    }

    fn outer_suffixes(&self) -> impl ExactSizeIterator<Item = &str> {
        let mut end = self.name.len();
        (self.suffixes.iter().rev()).map(move |&n| {
            end -= n;
            &self.name[end..end + n]
        })
    }

    fn renamed_alike(&self, other: &Obj, lax: bool) -> bool {
        ends_alike(self.outer_prefixes(), other.outer_prefixes(), lax)
            && ends_alike(self.outer_suffixes(), other.outer_suffixes(), lax)
    }

    fn resolve(&self, objs: &[Obj], written: &[usize], named: &[usize]) -> Vec<usize> {
        let mut chosen = written.to_vec();
        for lax in [true, false] {
            if chosen.len() > 1 {
                chosen.retain(|&j| self.renamed_alike(&objs[j], lax));
            }
        }
        match chosen.len() {
            0 => named.to_vec(),
            1 => chosen,
            _ => Vec::new(),
        }
    }

    fn address(&self) -> String {
        let head = match self.group.is_empty() {
            true => self.kind.clone(),
            false => format!("{}.{}", self.kind, self.group),
        };
        match &self.ns {
            Some(ns) => format!("{head}/{ns}/{}", self.name),
            None => format!("{head}/{}", self.name),
        }
    }
}

#[derive(Debug, Clone)]
struct ImageRule {
    new_name: Option<String>,
    new_tag: Option<String>,
    digest: Option<String>,
}

#[derive(Debug, Default)]
struct Kustomization {
    prefix: String,
    suffix: String,
    namespace: Option<String>,
    images: HashMap<String, ImageRule>,
    entries: Vec<String>,
    components: usize,
    patches: Vec<String>,
}

fn at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, k| v.get(*k))
}

fn text<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    at(v, path)?.as_str()
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn strings(v: Option<&Value>) -> BTreeMap<String, String> {
    (v.and_then(Value::as_object).into_iter().flatten())
        .filter_map(|(k, v)| Some((k.clone(), scalar(v)?)))
        .collect()
}

fn list<'a>(v: &'a Value, path: &[&str]) -> impl Iterator<Item = &'a Value> {
    at(v, path)
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|a| a.iter())
}

fn plain(name: &str) -> bool {
    !name.is_empty() && !name.contains("{{")
}

fn template<'a>(value: &'a Value, kind: &str) -> Option<&'a Value> {
    if kind == "Pod" {
        return Some(value);
    }
    [
        &["spec", "template"][..],
        &["spec", "jobTemplate", "spec", "template"],
    ]
    .into_iter()
    .filter_map(|p| at(value, p))
    .find(|t| {
        ["containers", "initContainers"]
            .iter()
            .any(|k| at(t, &["spec", k]).is_some_and(Value::is_array))
    })
}

fn facts(value: &Value, kind: &str, group: &str) -> Facts {
    let mut facts = Facts::default();
    if matches!(kind, "ConfigMap" | "Secret") && group.is_empty() {
        let keys = if kind == "Secret" {
            &["data", "stringData"][..]
        } else {
            &["data"][..]
        };
        for key in keys {
            for (name, value) in value[*key].as_object().into_iter().flatten() {
                facts.data.insert(
                    name.clone(),
                    if kind == "Secret" {
                        None
                    } else {
                        value.as_str().and_then(crate::iac::environment::literal)
                    },
                );
            }
        }
    }
    if let Some(template) = template(value, kind) {
        facts.labels = Some(strings(at(template, &["metadata", "labels"])));
        let spec = &template["spec"];
        facts.account = ["serviceAccountName", "serviceAccount"]
            .into_iter()
            .find_map(|k| Some((k, text(spec, &[k])?)))
            .filter(|(_, n)| plain(n))
            .map(|(k, n)| (k, n.to_string()));
        let mut mount = |kind: &'static str, name: Option<&str>, via: String| {
            if let Some(name) = name.filter(|n| plain(n)) {
                facts.mounts.push((kind, name.into(), via));
            }
        };
        for key in ["containers", "initContainers", "ephemeralContainers"] {
            for c in list(spec, &[key]) {
                facts.environment.extend(environment::read(c));
                for e in list(c, &["env"]) {
                    let var = text(e, &["name"]).unwrap_or_default();
                    let via = format!("env {var}");
                    let from = &e["valueFrom"];
                    mount(
                        "ConfigMap",
                        text(from, &["configMapKeyRef", "name"]),
                        via.clone(),
                    );
                    mount("Secret", text(from, &["secretKeyRef", "name"]), via);
                }
                for e in list(c, &["envFrom"]) {
                    let via = "envFrom".to_string();
                    mount("ConfigMap", text(e, &["configMapRef", "name"]), via.clone());
                    mount("Secret", text(e, &["secretRef", "name"]), via);
                }
            }
        }
        for v in list(spec, &["volumes"]) {
            let name = text(v, &["name"]).unwrap_or_default();
            if name.len() > VOLUME_NAME_MAX {
                continue;
            }
            let via = format!("volume {name}");
            mount("ConfigMap", text(v, &["configMap", "name"]), via.clone());
            mount("Secret", text(v, &["secret", "secretName"]), via.clone());
            for s in list(v, &["projected", "sources"]) {
                mount("ConfigMap", text(s, &["configMap", "name"]), via.clone());
                mount("Secret", text(s, &["secret", "name"]), via.clone());
            }
        }
        for key in ["containers", "initContainers", "ephemeralContainers"] {
            for c in list(spec, &[key]) {
                if let Some(image) = text(c, &["image"]).filter(|i| plain(i)) {
                    let name = text(c, &["name"]).unwrap_or_default();
                    facts.images.push((name.to_string(), image.to_string()));
                }
            }
        }
    }
    if kind == "Service" && group.is_empty() {
        facts.selector = strings(at(value, &["spec", "selector"]));
    }
    if kind == "Ingress" {
        if matches!(group, "networking.k8s.io" | "extensions") {
            facts.ingress = routing::read(value);
        }
        let service = |backend: &Value| {
            text(backend, &["service", "name"])
                .or_else(|| text(backend, &["serviceName"]))
                .filter(|n| plain(n))
                .map(String::from)
        };
        for rule in list(value, &["spec", "rules"]) {
            let host = text(rule, &["host"]).unwrap_or_default();
            if host.len() > HOST_MAX {
                continue;
            }
            for p in list(rule, &["http", "paths"]) {
                let path = text(p, &["path"]).unwrap_or_default();
                if let Some(name) = service(&p["backend"]) {
                    facts.backends.push((name, format!("backend {host}{path}")));
                }
            }
        }
        for key in ["defaultBackend", "backend"] {
            if let Some(name) = at(value, &["spec", key]).and_then(service) {
                facts.backends.push((name, "defaultBackend".into()));
            }
        }
    }
    facts
}

type Span<'a> = &'a dyn Fn(Option<usize>, usize) -> Option<(usize, usize)>;

fn object(
    path: &str,
    (line, end_line): (usize, usize),
    block: &str,
    value: &Value,
    span: Span,
    mappings: &crate::iac::mapping::Config,
) -> Option<Obj> {
    let api = text(value, &["apiVersion"])?;
    let kind = text(value, &["kind"])?;
    let name = text(value, &["metadata", "name"]).filter(|n| plain(n))?;
    if api.starts_with("kustomize.config.k8s.io/") {
        return None;
    }
    let group = api.rsplit_once('/').map_or("", |(g, _)| g);
    let ns = match text(value, &["metadata", "namespace"]) {
        Some(ns) if !plain(ns) => return None,
        ns => ns,
    };
    let ns = ns
        .filter(|_| !cluster_scoped(kind, group))
        .map(String::from);
    let mut facts = facts(value, kind, group);
    facts.xp = crossplane::facts(value, kind, group, block, span, mappings);
    facts.custom = custom::read(value, kind, group, mappings);
    facts.topology = topology::read(value, kind, group);
    let weight = weight(&facts);
    Some(Obj {
        group: group.into(),
        kind: kind.into(),
        orig: name.into(),
        name: name.into(),
        prefixes: Vec::new(),
        suffixes: Vec::new(),
        moved: false,
        ns,
        path: path.into(),
        line,
        end_line,
        block: block.into(),
        facts,
        weight,
    })
}

fn kustomization(value: &Value) -> Kustomization {
    let mut k = Kustomization {
        prefix: text(value, &["namePrefix"]).unwrap_or_default().into(),
        suffix: text(value, &["nameSuffix"]).unwrap_or_default().into(),
        namespace: text(value, &["namespace"])
            .filter(|n| !n.is_empty())
            .map(String::from),
        ..Kustomization::default()
    };
    for key in ["resources", "bases", "components"] {
        if key == "components" {
            k.components = k.entries.len();
        }
        k.entries.extend(
            list(value, &[key])
                .filter_map(Value::as_str)
                .map(String::from),
        );
    }
    for image in list(value, &["images"]) {
        let get = |key: &str| at(image, &[key]).and_then(scalar);
        if let Some(name) = get("name") {
            k.images.entry(name).or_insert_with(|| ImageRule {
                new_name: get("newName"),
                new_tag: get("newTag"),
                digest: get("digest"),
            });
        }
    }
    k.patches.extend(
        list(value, &["patchesStrategicMerge"])
            .filter_map(Value::as_str)
            .map(String::from),
    );
    for key in ["patches", "patchesJson6902"] {
        k.patches.extend(
            list(value, &[key])
                .filter_map(|p| text(p, &["path"]))
                .map(String::from),
        );
    }
    k
}

#[derive(Default)]
struct Parsed {
    objects: Vec<Obj>,
    kustomization: Option<Kustomization>,
}

fn resource(path: &[Step]) -> Option<(Option<usize>, usize)> {
    use Step::{Index, Key};
    let key = |s: &Step, k: &str| matches!(s, Key(x) if x == k);
    match path {
        [spec, res, Index(i), ..] if key(spec, "spec") && key(res, "resources") => Some((None, *i)),
        [spec, pipe, Index(j), input, res, Index(i), ..]
            if key(spec, "spec")
                && key(pipe, "pipeline")
                && key(input, "input")
                && key(res, "resources") =>
        {
            Some((Some(*j), *i))
        }
        _ => None,
    }
}

fn read(path: &str, src: &[u8], mappings: &crate::iac::mapping::Config) -> Parsed {
    let mut parsed = Parsed::default();
    if src.is_empty() || std::str::from_utf8(src).is_err() {
        return parsed;
    }
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut resources: HashMap<(usize, Option<usize>, usize), (usize, usize)> = HashMap::new();
    let widen = |span: &mut (usize, usize), line: usize| {
        (span.0, span.1) = (span.0.min(line), span.1.max(line));
    };
    let Ok(documents) = events::read(Syntax::of(path), src, |s| {
        if s.document < DOCUMENTS_MAX {
            if spans.len() <= s.document {
                spans.resize(s.document + 1, (s.line, s.line));
            }
            widen(&mut spans[s.document], s.line);
            if let Some((step, i)) = resource(s.path) {
                let span = resources.entry((s.document, step, i));
                widen(span.or_insert((s.line, s.line)), s.line);
            }
        }
    }) else {
        return parsed;
    };
    if documents.len() > DOCUMENTS_MAX {
        return parsed;
    }
    if is_kustomization(path) {
        parsed.kustomization = Some(kustomization(documents.first().unwrap_or(&Value::Null)));
        return parsed;
    }
    let many = documents.len() > 1;
    for (n, value) in documents.iter().enumerate() {
        let lines = spans.get(n).copied().unwrap_or((1, 1));
        let block = if many {
            format!("[{n}]")
        } else {
            String::new()
        };
        let span = |step: Option<usize>, i: usize| resources.get(&(n, step, i)).copied();
        parsed
            .objects
            .extend(object(path, lines, &block, value, &span, mappings));
    }
    parsed
}

fn retag(image: &str, rules: &HashMap<String, ImageRule>) -> String {
    let (rest, digest) = match image.split_once('@') {
        Some((rest, digest)) => (rest, Some(digest)),
        None => (image, None),
    };
    let (name, tag) = match rest.rfind(':').filter(|&i| !rest[i..].contains('/')) {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let Some(rule) = rules.get(name) else {
        return image.to_string();
    };
    let name = rule.new_name.as_deref().unwrap_or(name);
    match (&rule.digest, &rule.new_tag) {
        (Some(digest), _) => format!("{name}@{digest}"),
        (None, Some(tag)) => format!("{name}:{tag}"),
        (None, None) => {
            let tag = tag.map_or(String::new(), |t| format!(":{t}"));
            let digest = digest.map_or(String::new(), |d| format!("@{d}"));
            format!("{name}{tag}{digest}")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Image {
    pub root: String,
    pub address: String,
    pub container: String,
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unresolved {
    pub root: String,
    pub path: String,
    pub entry: String,
    pub reason: &'static str,
}

#[derive(Debug, Default)]
pub(crate) struct Output {
    pub nodes: Vec<Node>,
    pub references: Vec<Reference>,
    pub images: Vec<Image>,
    pub unresolved: Vec<Unresolved>,
}

enum Entry<'m> {
    File(&'m str, &'m [Obj]),
    Dir(&'m str),
    Unresolved(&'static str),
}

struct Eval<'m> {
    files: &'m BTreeMap<String, Parsed>,
    dirs: BTreeMap<&'m str, (&'m str, &'m Kustomization)>,
    root: String,
    room: usize,
    bytes: usize,
    steps: usize,
    reached: HashSet<&'m str>,
    root_left: usize,
    left: usize,
    kinds: crossplane::Kinds,
    out: Output,
}

fn remote(entry: &str) -> bool {
    entry.contains("://")
        || entry.starts_with("git@")
        || entry.contains("?ref=")
        || entry.starts_with("github.com/")
        || entry.starts_with("gitlab.com/")
        || entry.starts_with("bitbucket.org/")
}

impl<'m> Eval<'m> {
    fn charge(&mut self, bytes: usize) -> bool {
        match (
            self.root_left.checked_sub(bytes),
            self.left.checked_sub(bytes),
        ) {
            (Some(root), Some(all)) => {
                (self.root_left, self.left) = (root, all);
                true
            }
            _ => {
                self.root_left = 0;
                false
            }
        }
    }

    fn entry(&self, dir: &str, entry: &str) -> Entry<'m> {
        if remote(entry) {
            return Entry::Unresolved("remote");
        }
        let Some(path) = joined(dir, entry) else {
            return Entry::Unresolved("outside");
        };
        let files: &'m BTreeMap<String, Parsed> = self.files;
        if let Some(parsed) = files.get(&path) {
            let key = files.get_key_value(&path).map_or("", |(k, _)| k.as_str());
            return Entry::File(key, &parsed.objects);
        }
        match self.dirs.get_key_value(path.as_str()) {
            Some((dir, _)) => Entry::Dir(dir),
            None => Entry::Unresolved("missing"),
        }
    }

    fn objects(
        &mut self,
        dir: &'m str,
        depth: usize,
        stack: &mut Vec<&'m str>,
        mut objs: Vec<Obj>,
    ) -> Vec<Obj> {
        let (path, k) = self.dirs[dir];
        let mut listed: HashSet<&'m str> = HashSet::new();
        for (i, entry) in k.entries.iter().enumerate() {
            if self.steps == 0 {
                break;
            }
            self.steps -= 1;
            let found = match self.entry(dir, entry) {
                Entry::File(file, _) if !listed.insert(file) => continue,
                Entry::Dir(base)
                    if listed.contains(base) || i < k.components && !listed.insert(base) =>
                {
                    continue;
                }
                Entry::File(file, objects) => {
                    self.reached.insert(file);
                    let take = self.fit(objects);
                    objs.extend(objects[..take].iter().cloned());
                    continue;
                }
                Entry::Dir(base) if stack.contains(&base) => Entry::Unresolved("cycle"),
                Entry::Dir(_) if depth >= OVERLAY_DEPTH_MAX => Entry::Unresolved("depth"),
                found => found,
            };
            match found {
                Entry::Dir(base) => {
                    stack.push(base);
                    let given = match i >= k.components {
                        true => {
                            self.steps = self.steps.saturating_sub(objs.len());
                            std::mem::take(&mut objs)
                        }
                        false => Vec::new(),
                    };
                    objs.extend(self.objects(base, depth + 1, stack, given));
                    stack.pop();
                }
                Entry::Unresolved(reason) => {
                    if self.charge(entry.len() + path.len() + self.root.len()) {
                        self.out.unresolved.push(Unresolved {
                            root: self.root.clone(),
                            path: path.into(),
                            entry: entry.clone(),
                            reason,
                        });
                    }
                }
                Entry::File(..) => {}
            }
        }
        environment::transform(&mut objs, &k.prefix, &k.suffix, k.namespace.as_deref());
        for o in &mut objs {
            if !(k.prefix.is_empty() && k.suffix.is_empty()
                || UNPREFIXED.contains(&o.kind.as_str()))
            {
                o.name = format!("{}{}{}", k.prefix, o.name, k.suffix);
                (o.prefixes
                    .extend((!k.prefix.is_empty()).then_some(k.prefix.len())));
                (o.suffixes
                    .extend((!k.suffix.is_empty()).then_some(k.suffix.len())));
                o.moved = true;
            }
            if let Some(ns) = &k.namespace {
                o.moved = true;
                if !cluster_scoped(&o.kind, &o.group) {
                    o.ns = Some(ns.clone());
                }
            }
            for (_, image) in &mut o.facts.images {
                *image = retag(image, &k.images);
            }
        }
        objs
    }

    fn unit(&mut self, mut objs: Vec<Obj>) {
        objs.truncate(OBJECTS_MAX);
        environment::transform(&mut objs, "", "", None);
        objs.sort_by_cached_key(Obj::address);
        let duplicates = objs
            .windows(2)
            .filter(|pair| pair[0].address() == pair[1].address())
            .map(|pair| pair[0].address())
            .collect::<HashSet<_>>();
        for object in &mut objs {
            if duplicates.contains(&object.address()) {
                if let Some(custom) = &mut object.facts.custom {
                    custom.invalidate(false);
                }
                if let Some(topology) = &mut object.facts.topology {
                    topology.invalidate();
                }
            }
        }
        let duplicate_services = objs
            .windows(2)
            .filter(|pair| {
                pair[0].kind == "Service"
                    && pair[0].group.is_empty()
                    && pair[0].address() == pair[1].address()
            })
            .map(|pair| pair[0].address())
            .collect::<HashSet<_>>();
        objs.dedup_by_key(|o| o.address());
        let addresses: Vec<String> = objs.iter().map(Obj::address).collect();
        let mut service_names = HashMap::<_, Vec<_>>::new();
        let mut services_named = HashMap::<_, Vec<_>>::new();
        for (j, (object, address)) in objs.iter().zip(&addresses).enumerate() {
            if object.kind == "Service"
                && object.group.is_empty()
                && !duplicate_services.contains(address)
            {
                let ns = nameref_ns(object.ns.as_deref());
                if object.moved {
                    (service_names.entry((ns, object.orig.as_str())).or_default()).push(j);
                }
                if object.name.len() <= OBJECT_NAME_MAX {
                    let key = (ns, object.name.as_str());
                    services_named.entry(key).or_default().push(j);
                }
            }
        }
        let roles: Vec<Option<crossplane::Role>> =
            objs.iter().map(|o| self.kinds.role(o)).collect();
        let configs = environment::configs(&objs);
        let customs = custom::Targets::new(&objs);
        let start = self.out.nodes.len();
        let references_start = self.out.references.len();
        let mut kept = 0;
        for (o, address) in objs.iter().zip(&addresses) {
            let environment = environment::resolve(o, &configs);
            let ingress = o
                .facts
                .ingress
                .iter()
                .map(|route| {
                    let key = (nameref_ns(o.ns.as_deref()), route.service.as_str());
                    let targets = o.resolve(
                        &objs,
                        service_names.get(&key).map_or(&[], Vec::as_slice),
                        services_named.get(&key).map_or(&[], Vec::as_slice),
                    );
                    (
                        route,
                        match targets[..] {
                            [service] => addresses[service].as_str(),
                            _ => "",
                        },
                    )
                })
                .collect::<Vec<_>>();
            if !self.charge(
                NODE_BYTES
                    + address.len()
                    + o.path.len()
                    + o.facts.custom.as_ref().map_or(0, |c| {
                        c.size(&o.block, &o.name) + self.root.len() + address.len()
                    })
                    + o.facts
                        .topology
                        .as_ref()
                        .map_or(0, |t| t.size() + o.block.len() * 3)
                    + ingress
                        .iter()
                        .map(|(r, s)| r.host.len() + r.prefix.len() + s.len())
                        .sum::<usize>()
                    + environment
                        .iter()
                        .map(crate::iac::environment::Entry::size)
                        .sum::<usize>(),
            ) {
                break;
            }
            kept += 1;
            let mut projection = o
                .facts
                .custom
                .as_ref()
                .map(|c| c.projection(&o.block, &o.name, o.ns.as_deref(), &customs));
            if let Some(topology) = &o.facts.topology {
                projection
                    .get_or_insert_default()
                    .defining
                    .extend(topology.defining(&o.block));
            }
            let subscription = o.facts.custom.as_ref().and_then(|c| {
                c.subscription(
                    projection.as_ref()?,
                    o.ns.as_deref(),
                    &customs,
                    address,
                    &self.root,
                )
            });
            self.out.nodes.push(Node {
                address: address.clone(),
                root: self.root.clone(),
                ty: o.kind.clone(),
                name: Some(
                    o.facts
                        .custom
                        .as_ref()
                        .and_then(|custom| custom.physical(&o.name))
                        .or_else(|| {
                            o.facts
                                .topology
                                .as_ref()
                                .and_then(topology::Facts::physical)
                        })
                        .map(String::from)
                        .unwrap_or_else(|| {
                            o.facts
                                .xp
                                .external
                                .clone()
                                .unwrap_or_else(|| o.name.clone())
                        }),
                ),
                pattern: false,
                overridable: Vec::new(),
                path: o.path.clone(),
                line: o.line,
                end_line: o.end_line,
                block: o.block.clone(),
                image: None,
                build: None,
                build_image: false,
                dockerfile: None,
                role: roles[kept - 1].map(|r| r.text().into()),
                handler: None,
                events: Vec::new(),
                routes: Vec::new(),
                subscription,
                environment,
                images: Vec::new(),
                provisioned: projection,
                ingress: ingress
                    .into_iter()
                    .map(|(route, service)| crate::iac::Ingress {
                        host: route.host.clone(),
                        prefix: route.prefix.clone(),
                        service: service.into(),
                    })
                    .collect(),
                actions: None,
            });
        }
        let (objs, addresses, roles) = (&objs[..kept], &addresses[..kept], &roles[..kept]);
        let customs = custom::Targets::new(objs);
        let mut composed: Vec<(usize, String, &crossplane::Base)> = Vec::new();
        let mut held: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (i, o) in objs.iter().enumerate() {
            for b in &o.facts.xp.bases {
                let address = format!("{}/{}", addresses[i], b.resource);
                let base_name = b.name.as_deref().unwrap_or("*");
                if !seen.insert(address.clone())
                    || !self.charge(
                        address.len()
                            + o.path.len()
                            + b.custom.as_ref().map_or(0, |c| {
                                c.size(&b.block, base_name) + address.len() + self.root.len() + 5
                            })
                            + b.topology
                                .as_ref()
                                .map_or(0, |t| t.size() + b.block.len() * 3 + 15),
                    )
                {
                    continue;
                }
                let mut projection = b.custom.as_ref().map(|c| {
                    c.projection(
                        &format!("{}.base", b.block),
                        base_name,
                        b.namespace.as_deref(),
                        &customs,
                    )
                });
                if let Some(topology) = &b.topology {
                    projection
                        .get_or_insert_default()
                        .defining
                        .extend(topology.defining(&format!("{}.base", b.block)));
                }
                let declared_name = b
                    .custom
                    .as_ref()
                    .and_then(|c| c.physical(base_name))
                    .or_else(|| b.topology.as_ref().and_then(topology::Facts::physical));
                let subscription = b.custom.as_ref().and_then(|c| {
                    c.subscription(
                        projection.as_ref()?,
                        b.namespace.as_deref(),
                        &customs,
                        &address,
                        &self.root,
                    )
                });
                self.out.nodes.push(Node {
                    address: address.clone(),
                    root: self.root.clone(),
                    ty: b.kind.1.clone(),
                    name: Some(declared_name.unwrap_or(base_name).into()),
                    pattern: declared_name.is_none() && b.name.is_none(),
                    overridable: Vec::new(),
                    path: o.path.clone(),
                    line: b.line,
                    end_line: b.end_line,
                    block: b.block.clone(),
                    image: None,
                    build: None,
                    build_image: false,
                    dockerfile: None,
                    role: Some(crossplane::Role::Composed.text().into()),
                    handler: None,
                    events: Vec::new(),
                    routes: Vec::new(),
                    subscription,
                    environment: Vec::new(),
                    images: Vec::new(),
                    provisioned: projection,
                    ingress: Vec::new(),
                    actions: None,
                });
                held.entry(i).or_default().push(composed.len());
                composed.push((i, address, b));
            }
        }
        let crossplane = crossplane::Targets::new(objs);
        // ponytail: the name as written is its only previous name, so a reference by the name a
        // middle stage gave names the object called so; keep each stage's name if overlays do.
        let mut written: HashMap<(&str, &str, Option<&str>), Vec<usize>> = HashMap::new();
        let mut named: HashMap<(&str, &str, Option<&str>), Vec<usize>> = HashMap::new();
        let mut labelled: HashMap<(Option<&str>, &str, &str), Vec<usize>> = HashMap::new();
        for (i, o) in objs.iter().enumerate() {
            if o.group.is_empty() {
                if o.moved && !matches!(o.kind.as_str(), "ConfigMap" | "Secret") {
                    let key = (
                        o.kind.as_str(),
                        o.orig.as_str(),
                        nameref_ns(o.ns.as_deref()),
                    );
                    written.entry(key).or_default().push(i);
                }
                if o.name.len() <= OBJECT_NAME_MAX
                    || matches!(o.kind.as_str(), "ConfigMap" | "Secret")
                {
                    let key = (
                        o.kind.as_str(),
                        o.name.as_str(),
                        nameref_ns(o.ns.as_deref()),
                    );
                    named.entry(key).or_default().push(i);
                }
            }
            for (k, v) in o.facts.labels.iter().flatten() {
                labelled.entry((o.ns.as_deref(), k, v)).or_default().push(i);
            }
        }
        for (i, o) in objs.iter().enumerate() {
            let mut targets: BTreeSet<(&str, String)> = BTreeSet::new();
            let ns = o.ns.as_deref();
            let find = |kind: &str, name: &str, renamed: bool| {
                let key = (kind, name, nameref_ns(ns));
                let written = written.get(&key).filter(|_| renamed);
                o.resolve(
                    objs,
                    written.map_or(&[], Vec::as_slice),
                    named.get(&key).map_or(&[], Vec::as_slice),
                )
            };
            if !duplicate_services.contains(&addresses[i])
                && let Some((k, v)) = o.facts.selector.iter().next()
            {
                let selector = (o.facts.selector.iter())
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let candidates = labelled.get(&(ns, k.as_str(), v.as_str()));
                let mut matched = 0;
                for &j in candidates.into_iter().flatten() {
                    if matched == OBJECT_REFERENCES_MAX {
                        break;
                    }
                    let labels = objs[j].facts.labels.as_ref();
                    if labels
                        .is_some_and(|l| o.facts.selector.iter().all(|(k, v)| l.get(k) == Some(v)))
                    {
                        targets.insert((&addresses[j], format!("selector {selector}")));
                        matched += 1;
                    }
                }
            }
            for (kind, name, via) in &o.facts.mounts {
                for j in find(kind, name, true) {
                    targets.insert((&addresses[j], via.clone()));
                }
            }
            if let Some((field, name)) = &o.facts.account {
                let renamed =
                    *field == "serviceAccountName" && ACCOUNT_RENAMED.contains(&o.kind.as_str());
                for j in find("ServiceAccount", name, renamed) {
                    targets.insert((&addresses[j], (*field).into()));
                }
            }
            for (name, via) in &o.facts.backends {
                for j in find("Service", name, true) {
                    targets.insert((&addresses[j], via.clone()));
                }
            }
            let mut found = BTreeSet::new();
            crossplane.of(o, roles[i], &mut found);
            for (j, via) in found {
                targets.insert((&addresses[j], via));
            }
            if let Some(mapped) = &o.facts.custom {
                for (j, via) in customs.references(mapped, ns) {
                    targets.insert((&addresses[j], via));
                }
            }
            let own = held.get(&i).map_or(&[][..], Vec::as_slice);
            for &c in own {
                let (_, address, b) = &composed[c];
                targets.insert((address, b.via.clone()));
            }
            for (target, via) in targets
                .into_iter()
                .filter(|(t, _)| *t != addresses[i])
                .take(OBJECT_REFERENCES_MAX + own.len())
            {
                let size = addresses[i].len() + target.len() + via.len();
                if !self.charge(size) {
                    return;
                }
                self.out.references.push(Reference {
                    root: self.root.clone(),
                    source: addresses[i].clone(),
                    target: target.into(),
                    via: Some(via),
                });
            }
            for (container, image) in &o.facts.images {
                if !self.charge(addresses[i].len() + container.len() + image.len()) {
                    return;
                }
                self.out.images.push(Image {
                    root: self.root.clone(),
                    address: addresses[i].clone(),
                    container: container.clone(),
                    image: image.clone(),
                });
            }
        }
        for (_, address, b) in &composed {
            let mut targets = crossplane.of_base(b).into_iter().collect::<BTreeSet<_>>();
            if let Some(mapped) = &b.custom {
                targets.extend(customs.references(mapped, b.namespace.as_deref()));
            }
            for (j, via) in targets.into_iter().take(OBJECT_REFERENCES_MAX) {
                if !self.charge(address.len() + addresses[j].len() + via.len()) {
                    return;
                }
                self.out.references.push(Reference {
                    root: self.root.clone(),
                    source: address.clone(),
                    target: addresses[j].clone(),
                    via: Some(via),
                });
            }
        }
        let views = objs
            .iter()
            .zip(addresses)
            .enumerate()
            .filter_map(|(i, (o, address))| {
                Some(topology::View {
                    slot: start + i,
                    address,
                    namespace: o.ns.as_deref(),
                    facts: o.facts.topology.as_ref()?,
                })
            })
            .chain(
                composed
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (_, address, base))| {
                        Some(topology::View {
                            slot: start + kept + i,
                            address,
                            namespace: base.namespace.as_deref(),
                            facts: base.topology.as_ref()?,
                        })
                    }),
            )
            .collect::<Vec<_>>();
        if views.is_empty() {
            return;
        }
        let topology = topology::Targets::new(&views);
        let mut references = HashMap::<&str, usize>::new();
        for reference in &self.out.references[references_start..] {
            *references.entry(&reference.source).or_default() += 1;
        }
        let mut references = references
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<HashMap<_, _>>();
        for (i, view) in views.iter().enumerate() {
            let prepared = topology.of(i);
            for (target, via) in prepared.references {
                let count = references.entry(view.address.into()).or_default();
                if *count >= OBJECT_REFERENCES_MAX {
                    break;
                }
                let target = views[target].address;
                if !self.charge(view.address.len() + target.len() + via.len()) {
                    return;
                }
                *count += 1;
                self.out.references.push(Reference {
                    root: self.root.clone(),
                    source: view.address.into(),
                    target: target.into(),
                    via: Some(via.into()),
                });
            }
            if let Some((name, exchange, pattern)) = prepared.binding {
                if !self.charge(name.len() + exchange.len() + pattern.len() + 16) {
                    return;
                }
                self.out.nodes[view.slot]
                    .provisioned
                    .get_or_insert_default()
                    .binding = Some(crate::iac::mapping::Binding {
                    contract: crate::channel::contract("amqp", Some(name)),
                    exchange: exchange.into(),
                    pattern: pattern.into(),
                });
            }
        }
    }

    fn fit(&mut self, objects: &[Obj]) -> usize {
        let mut take = 0;
        for o in objects.iter().take(self.room) {
            let Some(left) = self.bytes.checked_sub(o.weight) else {
                self.room = take;
                break;
            };
            self.bytes = left;
            take += 1;
        }
        self.room -= take;
        take
    }

    fn start(&mut self, label: &str) {
        self.root = label.to_string();
        self.room = OBJECTS_MAX;
        self.bytes = OVERLAY_BYTES_MAX;
        self.steps = WALK_MAX;
        self.root_left = UNIT_STORED_MAX;
    }
}

#[cfg(test)]
pub(crate) fn evaluate(files: &[(String, Vec<u8>)]) -> Output {
    evaluate_with(files, &crate::iac::mapping::Config::default())
}

pub(crate) fn evaluate_with(
    files: &[(String, Vec<u8>)],
    mappings: &crate::iac::mapping::Config,
) -> Output {
    let parsed: BTreeMap<String, Parsed> = (files.iter())
        .map(|(path, src)| (path.clone(), read(path, src, mappings)))
        .collect();
    let dirs: BTreeMap<&str, (&str, &Kustomization)> = (parsed.iter())
        .filter_map(|(path, p)| {
            Some((
                config_values::parent(path),
                (path.as_str(), p.kustomization.as_ref()?),
            ))
        })
        .collect();
    let mut eval = Eval {
        files: &parsed,
        dirs,
        root: String::new(),
        room: 0,
        bytes: 0,
        steps: 0,
        reached: HashSet::new(),
        root_left: 0,
        left: KUBE_STORED_MAX,
        kinds: crossplane::Kinds::of(parsed.values().flat_map(|p| &p.objects)),
        out: Output::default(),
    };
    let mut called: HashSet<&str> = HashSet::new();
    let mut consumed: HashSet<&str> = HashSet::new();
    for (&dir, (_, k)) in &eval.dirs {
        for entry in &k.entries {
            if let Entry::Dir(base) = eval.entry(dir, entry)
                && base != dir
            {
                called.insert(base);
            }
        }
        for patch in &k.patches {
            consumed.extend(
                joined(dir, patch)
                    .and_then(|p| parsed.get_key_value(&p))
                    .map(|(k, _)| k.as_str()),
            );
        }
    }
    let roots: Vec<&str> = (eval.dirs.keys().copied())
        .filter(|d| !called.contains(d))
        .collect();
    for dir in roots {
        eval.start(if dir.is_empty() { "." } else { dir });
        let objs = eval.objects(dir, 0, &mut vec![dir], Vec::new());
        eval.unit(objs);
    }
    consumed.extend(eval.reached.iter().copied());
    let mut plain: BTreeMap<&str, Vec<Obj>> = BTreeMap::new();
    for (path, p) in &parsed {
        if !consumed.contains(path.as_str()) {
            plain
                .entry(config_values::parent(path))
                .or_default()
                .extend(p.objects.iter().cloned());
        }
    }
    for (dir, objs) in plain.into_iter().filter(|(_, o)| !o.is_empty()) {
        eval.start(if dir.is_empty() { "." } else { dir });
        eval.unit(objs);
    }
    eval.out
}

pub(crate) fn changed(tx: &Transaction, path: &str, src: Option<&[u8]>) -> Result<bool> {
    if !is_source(path) {
        return Ok(false);
    }
    if src.map_or_else(|| is_kustomization(path), |src| wanted(path, src)) {
        return Ok(true);
    }
    let held = format!(
        "SELECT 1 FROM iac_nodes WHERE path = ?1 AND {} LIMIT 1",
        crate::iac::Owner::Kubernetes.sql()
    );
    let held = tx.prepare_cached(&held)?.exists(params![path])?;
    Ok(held)
}

pub(crate) fn refresh(tx: &Transaction, output: &Output) -> Result<()> {
    crate::iac::forget(tx, None, crate::iac::Owner::Kubernetes)?;
    crate::iac::insert(tx, &output.nodes, &output.references)?;
    tx.execute_batch("DELETE FROM iac_images; DELETE FROM iac_unresolved;")?;
    let mut insert = tx.prepare_cached("INSERT INTO iac_images VALUES (?1, ?2, ?3, ?4)")?;
    for i in &output.images {
        insert.execute(params![i.root, i.address, i.container, i.image])?;
    }
    let mut insert = tx.prepare_cached("INSERT INTO iac_unresolved VALUES (?1, ?2, ?3, ?4)")?;
    for u in &output.unresolved {
        insert.execute(params![u.root, u.path, u.entry, u.reason])?;
    }
    Ok(())
}

pub(crate) fn images(db: &Connection, root: &str, address: &str) -> Result<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT container, image FROM iac_images WHERE root = ?1 AND address = ?2 ORDER BY rowid",
    )?;
    let rows = statement.query_map(params![root, address], |r| {
        Ok(format!(
            "{}: {}",
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn status(db: &Connection) -> Result<Option<String>> {
    let (count, remote): (i64, i64) = db.query_row(
        "SELECT count(*), coalesce(sum(reason = 'remote'), 0) FROM iac_unresolved",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((count > 0).then(|| {
        Msg::KustomizeUnresolved {
            count: count as usize,
            remote: remote as usize,
        }
        .text()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
        (list.iter())
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect()
    }

    fn addresses(out: &Output) -> Vec<String> {
        let mut a: Vec<String> = (out.nodes.iter())
            .map(|n| format!("{} [{}]", n.address, n.root))
            .collect();
        a.sort();
        a
    }

    fn shown(out: &Output) -> Vec<String> {
        let mut r: Vec<String> = (out.references.iter())
            .map(|r| {
                format!(
                    "{} -> {} via {}",
                    r.source,
                    r.target,
                    r.via.as_deref().unwrap_or("")
                )
            })
            .collect();
        r.sort();
        r
    }

    const DEPLOYMENT: &str = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: api\n  \
        namespace: shop\nspec:\n  selector:\n    matchLabels:\n      app: api\n  template:\n    \
        metadata:\n      labels:\n        app: api\n    spec:\n      serviceAccountName: runner\n      \
        containers:\n        - name: web\n          image: ghcr.io/acme/api:1.0\n          env:\n            \
        - name: DB\n              valueFrom:\n                secretKeyRef:\n                  \
        name: creds\n                  key: url\n          envFrom:\n            - configMapRef:\n                \
        name: settings\n      volumes:\n        - name: conf\n          configMap:\n            name: settings\n";

    const REST: &str = "apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n  namespace: shop\n\
        spec:\n  selector:\n    app: api\n---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  \
        name: settings\n  namespace: shop\n---\napiVersion: v1\nkind: Secret\nmetadata:\n  \
        name: creds\n  namespace: shop\n---\napiVersion: v1\nkind: ServiceAccount\nmetadata:\n  \
        name: runner\n  namespace: shop\n---\napiVersion: networking.k8s.io/v1\nkind: Ingress\n\
        metadata:\n  name: edge\n  namespace: shop\nspec:\n  rules:\n    - host: shop.example.com\n      \
        http:\n        paths:\n          - path: /api\n            pathType: Prefix\n            backend:\n              \
        service:\n                name: api\n  defaultBackend:\n    service:\n      name: api\n";

    #[test]
    fn an_object_is_a_node_at_kind_group_namespace_and_name() {
        let out = evaluate(&files(&[
            ("k/deploy.yaml", DEPLOYMENT),
            ("k/rest.yaml", REST),
            (
                "k/rbac.yaml",
                "apiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRole\nmetadata:\n  \
                 name: reader\n  namespace: ignored\n---\napiVersion: v1\nkind: ConfigMap\n\
                 metadata:\n  name: loose\n",
            ),
        ]));
        assert_eq!(
            addresses(&out),
            [
                "ClusterRole.rbac.authorization.k8s.io/reader [k]",
                "ConfigMap/loose [k]",
                "ConfigMap/shop/settings [k]",
                "Deployment.apps/shop/api [k]",
                "Ingress.networking.k8s.io/shop/edge [k]",
                "Secret/shop/creds [k]",
                "Service/shop/api [k]",
                "ServiceAccount/shop/runner [k]",
            ]
        );
        let deployment = (out.nodes.iter()).find(|n| n.ty == "Deployment").unwrap();
        assert_eq!(deployment.name.as_deref(), Some("api"));
        assert_eq!(
            (deployment.path.as_str(), deployment.block.as_str()),
            ("k/deploy.yaml", "")
        );
        assert_eq!(deployment.line, 1);
        let secret = (out.nodes.iter()).find(|n| n.ty == "Secret").unwrap();
        assert_eq!(secret.block, "[2]");
    }

    #[test]
    fn selectors_mounts_accounts_and_backends_are_references_within_a_namespace() {
        let out = evaluate(&files(&[
            ("k/deploy.yaml", DEPLOYMENT),
            ("k/rest.yaml", REST),
            (
                "k/other.yaml",
                &REST.replace("namespace: shop", "namespace: other"),
            ),
        ]));
        let shop: Vec<String> = shown(&out)
            .into_iter()
            .filter(|r| r.contains("/shop/") && !r.contains("/other/"))
            .collect();
        assert_eq!(
            shop,
            [
                "Deployment.apps/shop/api -> ConfigMap/shop/settings via envFrom",
                "Deployment.apps/shop/api -> ConfigMap/shop/settings via volume conf",
                "Deployment.apps/shop/api -> Secret/shop/creds via env DB",
                "Deployment.apps/shop/api -> ServiceAccount/shop/runner via serviceAccountName",
                "Ingress.networking.k8s.io/shop/edge -> Service/shop/api via backend shop.example.com/api",
                "Ingress.networking.k8s.io/shop/edge -> Service/shop/api via defaultBackend",
                "Service/shop/api -> Deployment.apps/shop/api via selector app=api",
            ]
        );
        assert_eq!(
            out.references.len(),
            7 + 2,
            "the other namespace has its own"
        );
    }

    #[test]
    fn an_ingress_host_or_a_volume_name_past_its_ceiling_is_left_out() {
        let deploy = |volume: &str| {
            format!(
                "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: api\nspec:\n  template:\n    \
                 spec:\n      containers: []\n      volumes:\n        - name: {volume}\n          \
                 projected:\n            sources:\n              - configMap:\n                  \
                 name: settings\n"
            )
        };
        let ingress = |host: &str| {
            format!(
                "apiVersion: networking.k8s.io/v1\nkind: Ingress\nmetadata:\n  name: edge\nspec:\n  \
                 rules:\n    - host: {host}\n      http:\n        paths:\n          - path: /api\n            \
                 backend:\n              service:\n                name: api\n"
            )
        };
        let rest = "apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n---\napiVersion: v1\n\
                    kind: ConfigMap\nmetadata:\n  name: settings\n";
        let vias = |volume: &str, host: &str| {
            let out = evaluate(&files(&[
                ("k/deploy.yaml", &deploy(volume)),
                ("k/ingress.yaml", &ingress(host)),
                ("k/rest.yaml", rest),
            ]));
            let count = |head: &str| {
                (out.references.iter())
                    .filter(|r| r.via.as_deref().is_some_and(|v| v.starts_with(head)))
                    .count()
            };
            (count("volume "), count("backend "))
        };
        let (volume, host) = ("v".repeat(VOLUME_NAME_MAX), "h".repeat(HOST_MAX));
        assert_eq!(vias(&volume, &host), (1, 1));
        assert_eq!(vias(&format!("{volume}v"), &format!("{host}h")), (0, 0));
    }

    #[test]
    fn a_selector_needs_every_one_of_its_labels_and_an_empty_one_selects_nothing() {
        let svc = |selector: &str| {
            format!(
                "apiVersion: v1\nkind: Service\nmetadata:\n  name: s\nspec:\n  selector:{selector}\n"
            )
        };
        let deploy = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: d\nspec:\n  template:\n    \
            metadata:\n      labels:\n        app: a\n        tier: web\n    spec:\n      containers: []\n";
        let count = |selector: &str| {
            evaluate(&files(&[("a.yaml", deploy), ("b.yaml", &svc(selector))]))
                .references
                .len()
        };
        assert_eq!(count("\n    app: a"), 1);
        assert_eq!(count("\n    app: a\n    tier: web"), 1);
        assert_eq!(count("\n    app: a\n    tier: db"), 0);
        assert_eq!(count(" {}"), 0);
    }

    #[test]
    fn an_overlay_applies_prefix_suffix_namespace_and_images_over_its_bases() {
        let out = evaluate(&files(&[
            (
                "base/kustomization.yaml",
                "resources:\n  - deploy.yaml\n  - rest.yaml\nnamePrefix: b-\nimages:\n  - name: ghcr.io/acme/api\n    newTag: \"2.0\"\n",
            ),
            (
                "base/deploy.yaml",
                &DEPLOYMENT.replace("  namespace: shop\n", ""),
            ),
            (
                "base/rest.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: settings\n---\n\
                 apiVersion: v1\nkind: Namespace\nmetadata:\n  name: shop\n",
            ),
            (
                "overlays/dev/kustomization.yaml",
                "resources:\n  - ../../base\nnamespace: dev\nnamePrefix: dev-\nnameSuffix: -x\n\
                 images:\n  - name: ghcr.io/acme/api\n    newName: registry.example.com/api\n",
            ),
        ]));
        assert_eq!(
            addresses(&out),
            [
                "ConfigMap/dev/dev-b-settings-x [overlays/dev]",
                "Deployment.apps/dev/dev-b-api-x [overlays/dev]",
                "Namespace/shop [overlays/dev]",
            ]
        );
        assert_eq!(
            (out.images.iter())
                .map(|i| i.image.as_str())
                .collect::<Vec<_>>(),
            ["registry.example.com/api:2.0"]
        );
        assert_eq!(
            shown(&out),
            [
                "Deployment.apps/dev/dev-b-api-x -> ConfigMap/dev/dev-b-settings-x via envFrom",
                "Deployment.apps/dev/dev-b-api-x -> ConfigMap/dev/dev-b-settings-x via volume conf"
            ]
        );
    }

    #[test]
    fn a_component_changes_the_objects_of_the_kustomization_that_lists_it() {
        let out = evaluate(&files(&[
            ("base/kustomization.yaml", "resources:\n  - deploy.yaml\n"),
            (
                "base/deploy.yaml",
                &DEPLOYMENT.replace("  namespace: shop\n", ""),
            ),
            (
                "components/ha/kustomization.yaml",
                "apiVersion: kustomize.config.k8s.io/v1alpha1\nkind: Component\nresources:\n  \
                 - extra.yaml\nimages:\n  - name: ghcr.io/acme/api\n    newName: ghcr.io/acme/api-ha\n",
            ),
            (
                "components/ha/extra.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: settings\n",
            ),
            (
                "overlays/prod/kustomization.yaml",
                "components:\n  - ../../components/ha\nresources:\n  - ../../base\nnamespace: prod\nnamePrefix: p-\n",
            ),
        ]));
        assert_eq!(
            addresses(&out),
            [
                "ConfigMap/prod/p-settings [overlays/prod]",
                "Deployment.apps/prod/p-api [overlays/prod]",
            ]
        );
        assert_eq!(
            (out.images.iter())
                .map(|i| i.image.as_str())
                .collect::<Vec<_>>(),
            ["ghcr.io/acme/api-ha:1.0"]
        );
        let out = evaluate(&files(&[
            ("base/kustomization.yaml", "resources:\n  - extra.yaml\n"),
            (
                "base/extra.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: settings\n",
            ),
            (
                "ha/kustomization.yaml",
                "kind: Component\nnamePrefix: ha-\n",
            ),
            (
                "o/kustomization.yaml",
                "resources:\n  - ../base\ncomponents:\n  - ../ha\n  - ../ha\n",
            ),
        ]));
        assert_eq!(addresses(&out), ["ConfigMap/ha-ha-settings [o]"]);
        let config =
            |name: &str| format!("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {name}\n");
        let out = evaluate(&files(&[
            (
                "ha/kustomization.yaml",
                "kind: Component\nresources:\n  - own.yaml\nnamePrefix: ha-\n",
            ),
            ("ha/own.yaml", &config("own")),
            (
                "o/kustomization.yaml",
                "resources:\n  - cm.yaml\n  - ../ha\ncomponents:\n  - cm.yaml\n  - ../ha\n",
            ),
            ("o/cm.yaml", &config("mine")),
        ]));
        assert_eq!(
            addresses(&out),
            ["ConfigMap/ha-own [o]", "ConfigMap/mine [o]"]
        );
    }

    #[test]
    fn components_reached_along_many_paths_are_walked_within_the_entry_ceiling() {
        let mut list: Vec<(String, String)> = Vec::new();
        for level in 0..8 {
            for n in 0..3 {
                let next = (0..3)
                    .map(|m| format!("  - ../l{}n{m}\n", level + 1))
                    .collect::<String>();
                list.push((
                    format!("l{level}n{n}/kustomization.yaml"),
                    format!("kind: Component\ncomponents:\n{next}"),
                ));
            }
        }
        for n in 0..3 {
            list.push((
                format!("l8n{n}/kustomization.yaml"),
                "kind: Component\n".into(),
            ));
        }
        let objects: String = (0..100)
            .map(|i| format!("---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: c{i}\n"))
            .collect();
        list.push(("top/all.yaml".into(), objects));
        list.push((
            "last/kustomization.yaml".into(),
            "kind: Component\nnamePrefix: last-\n".into(),
        ));
        list.push((
            "top/kustomization.yaml".into(),
            "resources:\n  - all.yaml\ncomponents:\n  - ../l0n0\n  - ../last\n".into(),
        ));
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let named = addresses(&evaluate(&files(&list)));
        assert_eq!(named.len(), 100);
        assert!(
            named.iter().all(|a| !a.contains("last-")),
            "{:?}",
            &named[..2]
        );
    }

    #[test]
    fn a_reference_follows_the_prefix_of_the_base_that_holds_its_target() {
        let base = |prefix: &str| {
            format!("resources:\n  - deploy.yaml\n  - rest.yaml\nnamePrefix: {prefix}-\n")
        };
        let out = evaluate(&files(&[
            (
                "app/kustomization.yaml",
                "resources:\n  - ../frontend\n  - ../backend\n",
            ),
            ("frontend/kustomization.yaml", &base("frontend")),
            ("frontend/deploy.yaml", DEPLOYMENT),
            ("frontend/rest.yaml", REST),
            ("backend/kustomization.yaml", &base("backend")),
            ("backend/deploy.yaml", DEPLOYMENT),
            ("backend/rest.yaml", REST),
        ]));
        let named = (shown(&out).into_iter())
            .filter(|r| !r.ends_with("via selector app=api"))
            .collect::<Vec<_>>();
        assert_eq!(named.len(), 12, "{named:#?}");
        let crossed = (named.iter())
            .filter(|r| {
                let (source, target) = r.split_once(" -> ").unwrap();
                source.contains("/frontend-") != target.contains("/frontend-")
            })
            .collect::<Vec<_>>();
        assert!(crossed.is_empty(), "{crossed:#?}");
        let routes = (out.nodes.iter())
            .filter(|n| n.ty == "Ingress")
            .map(|n| {
                let services = n.ingress.iter().map(|i| i.service.as_str());
                (n.address.as_str(), services.collect::<BTreeSet<_>>())
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            routes,
            BTreeMap::from([
                (
                    "Ingress.networking.k8s.io/shop/backend-edge",
                    BTreeSet::from(["Service/shop/backend-api"])
                ),
                (
                    "Ingress.networking.k8s.io/shop/frontend-edge",
                    BTreeSet::from(["Service/shop/frontend-api"])
                ),
            ])
        );
    }

    #[test]
    fn a_reference_follows_the_only_object_written_with_its_name() {
        let out = evaluate(&files(&[
            (
                "app/kustomization.yaml",
                "resources:\n  - ../web\n  - ../accounts\nnamespace: shop\n",
            ),
            (
                "web/kustomization.yaml",
                "resources:\n  - deploy.yaml\n  - edge.yaml\nnamespace: a\n",
            ),
            ("web/deploy.yaml", DEPLOYMENT),
            (
                "web/edge.yaml",
                &REST[REST.find("apiVersion: networking").unwrap()..],
            ),
            (
                "accounts/kustomization.yaml",
                "resources:\n  - rest.yaml\nnamePrefix: acct-\nnamespace: b\n",
            ),
            (
                "accounts/rest.yaml",
                &REST[..REST.find("---\napiVersion: networking").unwrap()],
            ),
        ]));
        let named = (shown(&out).into_iter())
            .filter(|r| r.contains("serviceAccountName") || r.starts_with("Ingress"))
            .collect::<Vec<_>>();
        assert_eq!(
            named,
            [
                "Deployment.apps/shop/api -> ServiceAccount/shop/acct-runner via serviceAccountName",
                "Ingress.networking.k8s.io/shop/edge -> Service/shop/acct-api via backend shop.example.com/api",
                "Ingress.networking.k8s.io/shop/edge -> Service/shop/acct-api via defaultBackend",
            ]
        );
        let edge = (out.nodes.iter()).find(|n| n.ty == "Ingress").unwrap();
        assert!(
            !edge.ingress.is_empty()
                && (edge.ingress.iter()).all(|route| route.service == "Service/shop/acct-api"),
            "{:?}",
            edge.ingress
        );
    }

    #[test]
    fn an_account_reference_kustomize_leaves_keeps_its_name_as_written() {
        let accounts = |deploy: &str| {
            let out = evaluate(&files(&[
                (
                    "app/kustomization.yaml",
                    "resources:\n  - deploy.yaml\n  - ../accounts\n",
                ),
                ("app/deploy.yaml", deploy),
                (
                    "accounts/kustomization.yaml",
                    "resources:\n  - rest.yaml\nnamePrefix: acct-\n",
                ),
                (
                    "accounts/rest.yaml",
                    &REST[..REST.find("---\napiVersion: networking").unwrap()],
                ),
            ]));
            (shown(&out).into_iter())
                .filter(|r| r.contains(" -> ServiceAccount/"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            accounts(DEPLOYMENT),
            ["Deployment.apps/shop/api -> ServiceAccount/shop/acct-runner via serviceAccountName"]
        );
        let replicas = DEPLOYMENT.replace("kind: Deployment", "kind: ReplicaSet");
        assert_eq!(accounts(&replicas), Vec::<String>::new());
        let deprecated = DEPLOYMENT.replace("serviceAccountName:", "serviceAccount:");
        assert_eq!(accounts(&deprecated), Vec::<String>::new());
    }

    #[test]
    fn a_reference_picks_among_renamed_objects_by_their_outermost_stages() {
        let kust = |resources: &[&str], prefix: &str| {
            let listed: String = resources.iter().map(|r| format!("  - {r}\n")).collect();
            match prefix {
                "" => format!("resources:\n{listed}"),
                _ => format!("resources:\n{listed}namePrefix: {prefix}\n"),
            }
        };
        let svc =
            "apiVersion: v1\nkind: Service\nmetadata:\n  name: x\nspec:\n  selector:\n    app: x\n";
        let edge = |target: &str| {
            format!(
                "apiVersion: networking.k8s.io/v1\nkind: Ingress\nmetadata:\n  name: edge\nspec:\n  \
                 rules:\n    - host: shop.example.com\n      http:\n        paths:\n          - path: /api\n            \
                 pathType: Prefix\n            backend:\n              service:\n                name: {target}\n"
            )
        };
        let routed = |tree: &[(&str, String)]| {
            let tree = (tree.iter())
                .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
                .collect::<Vec<_>>();
            let out = evaluate(&tree);
            let edge = (out.nodes.iter()).find(|n| n.ty == "Ingress").unwrap();
            let routes = (edge.ingress.iter())
                .map(|r| r.service.clone())
                .collect::<BTreeSet<_>>();
            let references = (out.references.iter())
                .filter(|r| r.source.starts_with("Ingress"))
                .map(|r| r.target.clone())
                .collect::<BTreeSet<_>>();
            (routes, references)
        };
        let to = |service: &str| {
            let one = BTreeSet::from([service.to_string()]);
            (one.clone(), one)
        };
        let none = (BTreeSet::from([String::new()]), BTreeSet::new());
        let nested = [
            ("top/kustomization.yaml", kust(&["../mid", "../c"], "")),
            ("mid/kustomization.yaml", kust(&["../b", "svc.yaml"], "a-")),
            ("mid/svc.yaml", svc.into()),
            ("b/kustomization.yaml", kust(&["edge.yaml"], "b-")),
            ("b/edge.yaml", edge("x")),
            ("c/kustomization.yaml", kust(&["svc.yaml"], "c-")),
            ("c/svc.yaml", svc.into()),
        ];
        assert_eq!(routed(&nested), to("Service/a-x"));
        let over = [
            ("top/kustomization.yaml", kust(&["../mid", "../b"], "")),
            ("mid/kustomization.yaml", kust(&["../a", "edge.yaml"], "m-")),
            ("mid/edge.yaml", edge("x")),
            ("a/kustomization.yaml", kust(&["svc.yaml"], "a-")),
            ("a/svc.yaml", svc.into()),
            ("b/kustomization.yaml", kust(&["svc.yaml"], "b-")),
            ("b/svc.yaml", svc.into()),
        ];
        assert_eq!(routed(&over), to("Service/m-a-x"));
        let joined = [
            (
                "top/kustomization.yaml",
                kust(&["../mid", "../p", "../c"], ""),
            ),
            ("mid/kustomization.yaml", kust(&["../r"], "a-")),
            ("r/kustomization.yaml", kust(&["edge.yaml"], "b-")),
            ("r/edge.yaml", edge("x")),
            ("p/kustomization.yaml", kust(&["svc.yaml"], "a-b-")),
            ("p/svc.yaml", svc.into()),
            ("c/kustomization.yaml", kust(&["svc.yaml"], "c-")),
            ("c/svc.yaml", svc.into()),
        ];
        assert_eq!(routed(&joined), none);
        let sibling = |second: &str| {
            [
                (
                    "top/kustomization.yaml",
                    kust(&["../r", "../a", "../n"], ""),
                ),
                ("r/kustomization.yaml", kust(&["edge.yaml"], "c-")),
                ("r/edge.yaml", edge("x")),
                ("a/kustomization.yaml", kust(&["svc.yaml"], "a-")),
                ("a/svc.yaml", svc.into()),
                ("n/kustomization.yaml", kust(&["svc.yaml"], second)),
                ("n/svc.yaml", svc.into()),
            ]
        };
        assert_eq!(routed(&sibling("b-")), none);
        assert_eq!(routed(&sibling("")), to("Service/a-x"));
        let written = |prefix: &str| {
            [
                ("top/kustomization.yaml", kust(&["../web", "edge.yaml"], "")),
                ("top/edge.yaml", edge(&format!("{prefix}x"))),
                ("web/kustomization.yaml", kust(&["svc.yaml"], prefix)),
                ("web/svc.yaml", svc.into()),
            ]
        };
        assert_eq!(routed(&written("frontend-")), to("Service/frontend-x"));
        let long = format!("{}-", "p".repeat(OBJECT_NAME_MAX));
        assert_eq!(routed(&written(&long)), none);
        let defaulted = [
            ("top/kustomization.yaml", kust(&["../r", "../a"], "")),
            (
                "r/kustomization.yaml",
                kust(&["edge.yaml"], "c-") + "namespace: default\n",
            ),
            ("r/edge.yaml", edge("x")),
            ("a/kustomization.yaml", kust(&["svc.yaml"], "a-")),
            ("a/svc.yaml", svc.into()),
        ];
        assert_eq!(routed(&defaulted), to("Service/a-x"));
        let out = evaluate(&files(&[
            (
                "o/kustomization.yaml",
                "resources:\n  - svc.yaml\nnamespace: \"\"\n",
            ),
            ("o/svc.yaml", svc),
        ]));
        assert_eq!(addresses(&out), ["Service/x [o]"]);
    }

    #[test]
    fn an_image_is_retagged_by_the_first_rule_that_names_it() {
        let rules = kustomization(&serde_json::json!({"images": [
            {"name": "nginx", "newTag": "1.25"},
            {"name": "reg:5000/app", "newName": "other/app"},
            {"name": "db", "newTag": 9, "digest": "sha256:abc"},
            {"name": "nginx", "newTag": "2"},
        ]}))
        .images;
        assert_eq!(retag("nginx", &rules), "nginx:1.25");
        assert_eq!(retag("nginx:1.0@sha256:x", &rules), "nginx:1.25");
        assert_eq!(retag("reg:5000/app:3", &rules), "other/app:3");
        assert_eq!(retag("db:1", &rules), "db@sha256:abc");
        assert_eq!(
            retag("reg:5000/app:3@sha256:x", &rules),
            "other/app:3@sha256:x"
        );
        assert_eq!(retag("reg:5000/web:3", &rules), "reg:5000/web:3");
    }

    #[test]
    fn each_overlay_is_its_own_unit_and_a_base_alone_is_none() {
        let out = evaluate(&files(&[
            ("base/kustomization.yaml", "resources:\n  - rest.yaml\n"),
            ("base/rest.yaml", REST),
            (
                "o/dev/kustomization.yaml",
                "resources:\n  - ../../base\nnamePrefix: dev-\n",
            ),
            ("o/prod/kustomization.yaml", "resources:\n  - ../../base\n"),
        ]));
        let roots: BTreeSet<&str> = out.nodes.iter().map(|n| n.root.as_str()).collect();
        assert_eq!(roots, BTreeSet::from(["o/dev", "o/prod"]));
        assert_eq!(out.nodes.len(), 10);
    }

    #[test]
    fn a_manifest_no_kustomization_lists_is_plain_and_a_patch_is_no_object() {
        let out = evaluate(&files(&[
            (
                "app/kustomization.yaml",
                "resources:\n  - deploy.yaml\npatches:\n  - path: patch.yaml\n",
            ),
            ("app/deploy.yaml", DEPLOYMENT),
            (
                "app/patch.yaml",
                "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: api\n",
            ),
            (
                "app/unlisted.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: loose\n",
            ),
            (
                "raw/a.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: one\n",
            ),
            (
                "raw/b.json",
                "{\"apiVersion\": \"v1\", \"kind\": \"ConfigMap\", \"metadata\": {\"name\": \"two\"}}",
            ),
            ("raw/ignore.yaml", "kind: Thing\n"),
            (
                "raw/template.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {{ .Release.Name }}\n",
            ),
        ]));
        assert_eq!(
            addresses(&out),
            [
                "ConfigMap/loose [app]",
                "ConfigMap/one [raw]",
                "ConfigMap/two [raw]",
                "Deployment.apps/shop/api [app]",
            ]
        );
    }

    #[test]
    fn a_helm_placeholder_in_a_namespace_or_an_image_is_no_object_or_image() {
        let out = evaluate(&files(&[
            (
                "a.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: literal\n  \
                 namespace: \"{{ .Values.ns }}\"\n",
            ),
            (
                "b.yaml",
                "apiVersion: v1\nkind: Pod\nmetadata:\n  name: p\nspec:\n  containers:\n    \
                 - name: c\n      image: \"{{ .Values.image }}\"\n    - name: d\n      image: nginx\n",
            ),
        ]));
        assert_eq!(addresses(&out), ["Pod/p [.]"]);
        assert_eq!(out.images.len(), 1);
        assert_eq!(out.images[0].image, "nginx");
    }

    #[test]
    fn a_kustomization_that_is_not_an_object_never_becomes_a_node() {
        let out = evaluate(&files(&[(
            "kustomization.yaml",
            "apiVersion: kustomize.config.k8s.io/v1beta1\nkind: Kustomization\nmetadata:\n  name: x\n",
        )]));
        assert!(out.nodes.is_empty());
    }

    #[test]
    fn a_base_that_is_remote_outside_or_missing_is_counted_unresolved() {
        let out = evaluate(&files(&[
            (
                "o/kustomization.yaml",
                "resources:\n  - github.com/acme/shared//kube?ref=v1\n  - https://example.com/x.yaml\n  \
                 - bitbucket.org/acme/shared//kube\n  - ../../../etc\n  - /abs\n  - gone.yaml\n  - ../a/b/../../o/self.yaml\n",
            ),
            (
                "o/self.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: ok\n",
            ),
        ]));
        let got: Vec<(&str, &str)> = (out.unresolved.iter())
            .map(|u| (u.entry.as_str(), u.reason))
            .collect();
        assert_eq!(
            got,
            [
                ("github.com/acme/shared//kube?ref=v1", "remote"),
                ("https://example.com/x.yaml", "remote"),
                ("bitbucket.org/acme/shared//kube", "remote"),
                ("../../../etc", "outside"),
                ("/abs", "outside"),
                ("gone.yaml", "missing"),
            ]
        );
        assert_eq!(addresses(&out), ["ConfigMap/ok [o]"]);
    }

    #[test]
    fn kustomizations_naming_each_other_are_followed_once_and_a_cycle_is_unresolved() {
        let out = evaluate(&files(&[
            (
                "root/kustomization.yaml",
                "resources:\n  - ../a\n  - cm.yaml\n",
            ),
            (
                "root/cm.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: r\n",
            ),
            ("a/kustomization.yaml", "resources:\n  - ../b\n"),
            ("b/kustomization.yaml", "resources:\n  - ../a\n"),
            (
                "x/kustomization.yaml",
                "resources:\n  - ../y\n  - lost.yaml\n",
            ),
            (
                "x/lost.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: lost\n",
            ),
            ("y/kustomization.yaml", "resources:\n  - ../x\n"),
        ]));
        assert_eq!(
            addresses(&out),
            ["ConfigMap/lost [x]", "ConfigMap/r [root]"]
        );
        let cycles: Vec<_> = out
            .unresolved
            .iter()
            .map(|u| (u.entry.as_str(), u.reason))
            .collect();
        assert_eq!(cycles, [("../a", "cycle")]);
    }

    #[test]
    fn bases_nested_past_the_depth_ceiling_are_unresolved() {
        let mut list = vec![(
            "d0/kustomization.yaml".to_string(),
            "resources:\n  - ../d1\n".to_string(),
        )];
        for i in 1..=OVERLAY_DEPTH_MAX + 2 {
            list.push((
                format!("d{i}/kustomization.yaml"),
                format!("resources:\n  - ../d{}\n  - cm.yaml\n", i + 1),
            ));
            list.push((
                format!("d{i}/cm.yaml"),
                format!("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: c{i}\n"),
            ));
        }
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let out = evaluate(&files(&list));
        let overlay = out.nodes.iter().filter(|n| n.root == "d0");
        assert_eq!(overlay.count(), OVERLAY_DEPTH_MAX);
        let past: Vec<_> = out.unresolved.iter().map(|u| u.reason).collect();
        assert_eq!(past, ["depth"]);
    }

    #[test]
    fn bases_reaching_one_another_along_many_paths_are_walked_within_the_entry_ceiling() {
        let mut list: Vec<(String, String)> = Vec::new();
        for level in 0..OVERLAY_DEPTH_MAX + 4 {
            for n in 0..3 {
                let next = (0..3)
                    .map(|m| format!("  - ../l{}n{m}\n", level + 1))
                    .collect::<String>();
                list.push((
                    format!("l{level}n{n}/kustomization.yaml"),
                    format!("resources:\n{next}"),
                ));
            }
        }
        list.push((
            "top/kustomization.yaml".into(),
            "resources:\n  - ../l0n0\n  - ../l0n1\n  - ../l0n2\n".into(),
        ));
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let out = evaluate(&files(&list));
        assert!(out.unresolved.len() < WALK_MAX, "{}", out.unresolved.len());
        assert!(
            out.unresolved
                .iter()
                .all(|u| u.reason == "depth" || u.reason == "cycle")
        );
    }

    #[test]
    fn an_overlay_past_the_object_ceiling_leaves_the_rest_out() {
        let docs: String = (0..OBJECTS_MAX + 5)
            .map(|i| format!("---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: c{i}\n"))
            .collect();
        let per = DOCUMENTS_MAX;
        let mut list: Vec<(String, String)> = vec![("kustomization.yaml".into(), String::new())];
        let mut resources = String::from("resources:\n");
        for (n, chunk) in docs
            .split("---\n")
            .filter(|c| !c.is_empty())
            .collect::<Vec<_>>()
            .chunks(per)
            .enumerate()
        {
            list.push((format!("f{n}.yaml"), chunk.join("---\n")));
            resources += &format!("  - f{n}.yaml\n");
        }
        list[0].1 = resources;
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let out = evaluate(&files(&list));
        assert_eq!(out.nodes.len(), OBJECTS_MAX);
    }

    #[test]
    fn a_repository_past_its_stored_budget_leaves_the_rest_out() {
        let long = "n".repeat(900);
        let base: String = (0..1000)
            .map(|i| {
                format!("---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {long}{i}\n")
            })
            .collect();
        let mut list: Vec<(String, String)> = vec![
            (
                "base/kustomization.yaml".into(),
                "resources:\n  - all.yaml\n".into(),
            ),
            ("base/all.yaml".into(), base),
        ];
        for i in 0..40 {
            list.push((
                format!("o{i}/kustomization.yaml"),
                "resources:\n  - ../base\n".into(),
            ));
        }
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let out = evaluate(&files(&list));
        let stored: usize = out
            .nodes
            .iter()
            .map(|n| n.address.len() + n.path.len())
            .sum();
        assert!(stored <= KUBE_STORED_MAX, "{stored}");
        assert!(out.nodes.len() < 40 * 1000, "{}", out.nodes.len());
        assert!(out.nodes.iter().any(|n| n.root == "o0"));
    }

    #[test]
    fn a_unit_past_its_stored_budget_leaves_the_rest_out() {
        let long = "n".repeat(900);
        let docs: String = (0..OBJECTS_MAX.min(DOCUMENTS_MAX))
            .map(|i| {
                format!("---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {long}{i}\n")
            })
            .collect();
        let mut list = vec![("kustomization.yaml".to_string(), "resources:\n".to_string())];
        for n in 0..8 {
            list[0].1 += &format!("  - f{n}.yaml\n");
            let changed = docs.replace(&long, &format!("n{n}x{long}"));
            list.push((format!("f{n}.yaml"), changed));
        }
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let out = evaluate(&files(&list));
        let stored: usize = out
            .nodes
            .iter()
            .map(|n| n.address.len() + n.path.len())
            .sum();
        assert!(stored <= UNIT_STORED_MAX, "{stored}");
        assert!(
            !out.nodes.is_empty() && out.nodes.len() < 8 * 1000,
            "{}",
            out.nodes.len()
        );
    }

    #[test]
    fn many_overlays_over_many_short_names_store_within_the_repository_budget() {
        let mut list = vec![(
            "base/kustomization.yaml".to_string(),
            "resources:\n".to_string(),
        )];
        for f in 0..OBJECTS_MAX / DOCUMENTS_MAX {
            list[0].1 += &format!("  - f{f}.yaml\n");
            let docs: String = (0..DOCUMENTS_MAX)
                .map(|i| {
                    format!("---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: f{f}c{i}\n")
                })
                .collect();
            list.push((format!("base/f{f}.yaml"), docs));
        }
        let overlays = KUBE_STORED_MAX / NODE_BYTES / OBJECTS_MAX + 2;
        for o in 0..overlays {
            list.push((
                format!("o{o}/kustomization.yaml"),
                "resources:\n  - ../base\n".into(),
            ));
        }
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let out = evaluate(&files(&list));
        let nodes = out.nodes.len();
        assert!(
            (OBJECTS_MAX..=KUBE_STORED_MAX / NODE_BYTES).contains(&nodes),
            "{nodes}"
        );
    }

    fn large_map(name: &str, entries: usize) -> String {
        let value = "v".repeat(1000);
        let data: String = (0..entries).map(|i| format!("  k{i}: {value}\n")).collect();
        format!("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {name}\ndata:\n{data}")
    }

    #[test]
    fn a_file_one_kustomization_names_twice_instantiates_once() {
        let resources = "  - big.yaml\n  - ./big.yaml\n".repeat(20) + "  - small.yaml\n";
        let out = evaluate(&files(&[
            ("o/kustomization.yaml", &format!("resources:\n{resources}")),
            ("o/big.yaml", &large_map("big", 600)),
            (
                "o/small.yaml",
                "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: small\n",
            ),
        ]));
        assert_eq!(
            addresses(&out),
            ["ConfigMap/big [o]", "ConfigMap/small [o]"]
        );
    }

    #[test]
    fn a_large_object_reached_along_many_paths_is_copied_within_the_byte_ceiling() {
        const LEVELS: usize = 6;
        let entries = 300;
        let mut list = vec![
            (format!("d{LEVELS}/map.yaml"), large_map("big", entries)),
            (
                format!("d{LEVELS}/kustomization.yaml"),
                "resources:\n  - map.yaml\n".to_string(),
            ),
        ];
        for level in 0..LEVELS {
            list.push((
                format!("d{level}/kustomization.yaml"),
                "resources:\n  - a\n  - b\n".into(),
            ));
            for side in ["a", "b"] {
                list.push((
                    format!("d{level}/{side}/kustomization.yaml"),
                    format!(
                        "namePrefix: {side}-\nresources:\n  - ../../d{}\n",
                        level + 1
                    ),
                ));
            }
        }
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let out = evaluate(&files(&list));
        let data: usize = (0..entries).map(|i| format!("k{i}").len() + 1000).sum();
        let copies = out.nodes.iter().filter(|n| n.root == "d0").count();
        assert!(copies > 0 && copies < 1 << LEVELS, "{copies}");
        assert!(copies * data <= OVERLAY_BYTES_MAX, "{copies}");
    }

    #[test]
    fn a_service_keeps_its_first_selected_workloads_past_the_reference_ceiling() {
        let mut list = vec![(
            "svc.yaml".to_string(),
            "apiVersion: v1\nkind: Service\nmetadata:\n  name: s\nspec:\n  selector:\n    app: a\n"
                .to_string(),
        )];
        for i in 0..OBJECT_REFERENCES_MAX + 6 {
            list.push((
                format!("d{i:03}.yaml"),
                format!("apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: d{i:03}\nspec:\n  template:\n    metadata:\n      labels:\n        app: a\n    spec:\n      containers: []\n"),
            ));
        }
        let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let out = evaluate(&files(&list));
        assert_eq!(out.references.len(), OBJECT_REFERENCES_MAX);
        assert!(
            out.references
                .iter()
                .any(|r| r.target == "Deployment.apps/d000")
        );
        assert!(
            out.references
                .iter()
                .all(|r| r.target != "Deployment.apps/d069")
        );
    }

    #[test]
    fn a_file_that_does_not_parse_or_holds_too_many_documents_adds_no_node() {
        let many = "---\na: 1\n".repeat(DOCUMENTS_MAX + 1);
        let out = evaluate(&files(&[
            ("bad.yaml", "apiVersion: v1\nkind: [\n"),
            (
                "many.yaml",
                &format!("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: x\n{many}"),
            ),
        ]));
        assert!(out.nodes.is_empty());
    }

    #[test]
    fn a_pod_and_a_cronjob_carry_their_labels_and_images() {
        let out = evaluate(&files(&[(
            "a.yaml",
            "apiVersion: v1\nkind: Pod\nmetadata:\n  name: p\n  labels:\n    app: p\nspec:\n  \
                 containers:\n    - name: c\n      image: nginx\n  initContainers:\n    - name: i\n      image: busybox:1\n---\n\
                 apiVersion: batch/v1\nkind: CronJob\nmetadata:\n  name: j\nspec:\n  jobTemplate:\n    spec:\n      template:\n        \
                 metadata:\n          labels:\n            app: j\n        spec:\n          containers:\n            - name: c\n              image: tool:2\n---\n\
                 apiVersion: v1\nkind: Service\nmetadata:\n  name: s\nspec:\n  selector:\n    app: j\n",
        )]));
        assert_eq!(
            shown(&out),
            ["Service/s -> CronJob.batch/j via selector app=j"]
        );
        let images: Vec<String> = out
            .images
            .iter()
            .map(|i| format!("{} {}: {}", i.address, i.container, i.image))
            .collect();
        assert_eq!(
            images,
            [
                "CronJob.batch/j c: tool:2",
                "Pod/p c: nginx",
                "Pod/p i: busybox:1"
            ]
        );
    }
}
