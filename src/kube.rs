//! Kubernetes objects and Kustomize trees as IaC nodes (#651, ADR 0025): every YAML or JSON
//! document with an `apiVersion`, a `kind` and a `metadata.name` is a node addressed
//! `<Kind>[.<group>]/<namespace>/<name>` (no namespace for a cluster-scoped kind, and none for an
//! object that has none), with the references between nodes stored like calls and the root that
//! roots its evaluation. They share the `iac_nodes` and `iac_references` tables, `search` kind
//! `iac`, `context` and `trace` with the Terraform nodes (`iac`). Never exported.
//!
//! A directory with a `kustomization.yaml` that no other kustomization names as a base is an
//! overlay, labelled by its path, and an evaluation unit: its `resources`, `bases` and
//! `components` that name a file or another kustomization directory inside the repository are
//! instantiated under it, base first, with each `kustomization.yaml` on the way applying
//! `namePrefix`, `nameSuffix`, `namespace` and `images` to what it holds. An entry that is
//! remote, outside the repository or not there is not followed and is counted as unresolved. A
//! manifest no kustomization lists (nor patches with) is a plain one: the manifests of a
//! directory are one unit, labelled by the directory.
//!
//! A Service selects the pod template owners whose pod labels hold its selector, a pod template
//! references the ConfigMaps and Secrets of its `env`, `envFrom` and volumes and its service
//! account, and an Ingress its backend Services, all in the same namespace and unit; a name is
//! matched as the object was written, before any prefix or suffix.

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

/// How deep kustomizations name each other as bases; a base deeper is not followed and is
/// counted as unresolved.
// ponytail: real trees nest a base or two under an overlay; 16 stops a chain built to run on.
const OVERLAY_DEPTH_MAX: usize = 16;

/// The objects one overlay instantiates, bases included; past it the rest of the overlay is left
/// out. A base named twice, or by many overlays' parents, instantiates once per name.
// ponytail: a large overlay holds a few thousand objects; 10,000 is past real ones.
const OBJECTS_MAX: usize = 10_000;

/// The entries of kustomizations one overlay walks, bases that name each other included; past it
/// the rest of the overlay is left out. Bases can reach one another along many paths (each of a
/// few entries naming a few more), which the depth ceiling alone leaves exponential.
// ponytail: a real overlay walks tens of entries; 50,000 is past any, and cheap.
const WALK_MAX: usize = 50_000;

/// The references one object keeps, in address order; past it the rest are left out.
// ponytail: a Service selects a handful of workloads; 64 is past a wide selector.
const OBJECT_REFERENCES_MAX: usize = 64;

/// The bytes of addresses, references, images and unresolved entries one unit may store; past it
/// the rest of the unit is left out.
// ponytail: four times the per-file ceiling holds a unit of a few thousand objects; past it the
// tree is built to explode.
const UNIT_STORED_MAX: usize = 4 * MAX_BYTES;

/// The same, across every unit of the repository; past it the rest is left out.
// ponytail: sixteen times the per-file ceiling holds a monorepo of overlays; many overlays over
// one exploding base would pass it.
const KUBE_STORED_MAX: usize = 16 * MAX_BYTES;

/// The kinds that live outside any namespace: a Kustomize `namespace` leaves them alone, and the
/// address has no namespace.
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

/// Whether objects of `kind` in `group` live outside any namespace: a kind of the fixed list, or
/// one of Crossplane's own (#655).
fn cluster_scoped(kind: &str, group: &str) -> bool {
    CLUSTER_SCOPED.contains(&kind) || crossplane::cluster_scoped(group)
}

/// The kinds a `namePrefix` and a `nameSuffix` leave alone, as Kustomize does.
const UNPREFIXED: &[&str] = &["APIService", "CustomResourceDefinition", "Namespace"];

/// Whether the file at `path` may hold objects: a YAML or JSON one.
pub(crate) fn is_source(path: &str) -> bool {
    [".yaml", ".yml", ".json"].iter().any(|e| path.ends_with(e))
}

fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Whether `path` is a kustomization file.
pub(crate) fn is_kustomization(path: &str) -> bool {
    matches!(file_name(path), "kustomization.yaml" | "kustomization.yml")
}

/// Whether the file `path` with bytes `src` is read for objects: a kustomization, or a file that
/// says `apiVersion` somewhere. The rest cannot hold an object.
pub(crate) fn wanted(path: &str, src: &[u8]) -> bool {
    is_source(path) && (is_kustomization(path) || src.windows(10).any(|w| w == b"apiVersion"))
}

/// What a pod template owner offers the references: its pod labels, and what its pod spec names.
#[derive(Debug, Clone, Default, PartialEq)]
struct Facts {
    /// The labels of its pod template (a Pod's own), `None` for an object that runs no pod.
    labels: Option<BTreeMap<String, String>>,
    /// A Service's selector.
    selector: BTreeMap<String, String>,
    /// The ConfigMaps and Secrets its pods mount: kind, name as written, and how it names it.
    mounts: Vec<(&'static str, String, String)>,
    account: Option<String>,
    /// An Ingress's backend Services: name as written and the rule that names it.
    backends: Vec<(String, String)>,
    /// Its containers' images: container name and image.
    images: Vec<(String, String)>,
    /// What it offers Crossplane's roles and references (#655).
    xp: crossplane::Facts,
}

/// One Kubernetes object, as read and as a kustomization on the way changed it.
#[derive(Debug, Clone, PartialEq)]
struct Obj {
    group: String,
    kind: String,
    /// The name as written, which the references to it use.
    orig: String,
    name: String,
    ns: Option<String>,
    path: String,
    line: usize,
    end_line: usize,
    block: String,
    facts: Facts,
}

impl Obj {
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

/// How a kustomization rewrites an image: the entries of its `images`.
#[derive(Debug, Clone)]
struct ImageRule {
    name: String,
    new_name: Option<String>,
    new_tag: Option<String>,
    digest: Option<String>,
}

/// The kustomization at `path`: what it names and what it changes.
#[derive(Debug, Default)]
struct Kustomization {
    prefix: String,
    suffix: String,
    namespace: Option<String>,
    images: Vec<ImageRule>,
    /// `resources`, `bases` and `components`, in order.
    entries: Vec<String>,
    /// The files its patches name, which are fragments and no objects.
    patches: Vec<String>,
}

fn at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, k| v.get(*k))
}

fn text<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    at(v, path)?.as_str()
}

/// A scalar as the text a label or an image tag shows.
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

/// A name that is a plain one: a Helm template's `{{ .Release.Name }}` is not.
fn plain(name: &str) -> bool {
    !name.is_empty() && !name.contains("{{")
}

/// The pod template of a workload (`metadata` and `spec`): a Pod's own, a `spec.template` or a
/// CronJob's `spec.jobTemplate.spec.template` whose spec holds containers.
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
    if let Some(template) = template(value, kind) {
        facts.labels = Some(strings(at(template, &["metadata", "labels"])));
        let spec = &template["spec"];
        facts.account = ["serviceAccountName", "serviceAccount"]
            .iter()
            .find_map(|k| text(spec, &[k]))
            .filter(|n| plain(n))
            .map(String::from);
        let mut mount = |kind: &'static str, name: Option<&str>, via: String| {
            if let Some(name) = name.filter(|n| plain(n)) {
                facts.mounts.push((kind, name.into(), via));
            }
        };
        for key in ["containers", "initContainers", "ephemeralContainers"] {
            for c in list(spec, &[key]) {
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
            let via = format!("volume {}", text(v, &["name"]).unwrap_or_default());
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
        let service = |backend: &Value| {
            text(backend, &["service", "name"])
                .or_else(|| text(backend, &["serviceName"]))
                .filter(|n| plain(n))
                .map(String::from)
        };
        for rule in list(value, &["spec", "rules"]) {
            let host = text(rule, &["host"]).unwrap_or_default();
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

/// The lines of a resource of a composition: its step in the pipeline, if any, and its index.
type Span<'a> = &'a dyn Fn(Option<usize>, usize) -> Option<(usize, usize)>;

/// The object a document is, or `None` when it is no object (a kustomization, a fragment with no
/// name, a Helm template's placeholder). `span` gives the lines of a composition's resources.
fn object(
    path: &str,
    (line, end_line): (usize, usize),
    block: &str,
    value: &Value,
    span: Span,
) -> Option<Obj> {
    let api = text(value, &["apiVersion"])?;
    let kind = text(value, &["kind"])?;
    let name = text(value, &["metadata", "name"]).filter(|n| plain(n))?;
    if api.starts_with("kustomize.config.k8s.io/") {
        return None;
    }
    let group = api.rsplit_once('/').map_or("", |(g, _)| g);
    // A namespace that is a template placeholder makes the document a Helm template, no object.
    let ns = match text(value, &["metadata", "namespace"]) {
        Some(ns) if !plain(ns) => return None,
        ns => ns,
    };
    let ns = ns
        .filter(|_| !cluster_scoped(kind, group))
        .map(String::from);
    let mut facts = facts(value, kind, group);
    facts.xp = crossplane::facts(value, kind, group, block, span);
    Some(Obj {
        group: group.into(),
        kind: kind.into(),
        orig: name.into(),
        name: name.into(),
        ns,
        path: path.into(),
        line,
        end_line,
        block: block.into(),
        facts,
    })
}

fn kustomization(value: &Value) -> Kustomization {
    let mut k = Kustomization {
        prefix: text(value, &["namePrefix"]).unwrap_or_default().into(),
        suffix: text(value, &["nameSuffix"]).unwrap_or_default().into(),
        namespace: text(value, &["namespace"]).map(String::from),
        ..Kustomization::default()
    };
    for key in ["resources", "bases", "components"] {
        k.entries.extend(
            list(value, &[key])
                .filter_map(Value::as_str)
                .map(String::from),
        );
    }
    for image in list(value, &["images"]) {
        let get = |key: &str| at(image, &[key]).and_then(scalar);
        if let Some(name) = get("name") {
            k.images.push(ImageRule {
                name,
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

/// What a file holds: its objects, and its kustomization when it is one.
#[derive(Default)]
struct Parsed {
    objects: Vec<Obj>,
    kustomization: Option<Kustomization>,
}

/// The resource of a composition the key path `path` lies under: its step in the pipeline (in
/// `spec.pipeline[j].input.resources[i]`), if any, and its index `i`.
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

/// Reads the file at `path`. A file that does not parse, holds more than `DOCUMENTS_MAX`
/// documents or is no UTF-8 holds nothing (config values count it unreadable); an empty `src` is
/// a file that exists and holds nothing.
fn read(path: &str, src: &[u8]) -> Parsed {
    let mut parsed = Parsed::default();
    if src.is_empty() || std::str::from_utf8(src).is_err() {
        return parsed;
    }
    // The first and last line of each document's scalars, and of each resource a composition of
    // the document may create (#655): its document, its step in the pipeline and its index.
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
            .extend(object(path, lines, &block, value, &span));
    }
    parsed
}

/// The image `image` becomes under the kustomization's rules: the first rule that names it.
fn retag(image: &str, rules: &[ImageRule]) -> String {
    let (rest, digest) = match image.split_once('@') {
        Some((rest, digest)) => (rest, Some(digest)),
        None => (image, None),
    };
    // A colon after the last slash is a tag; before it, a registry's port.
    let (name, tag) = match rest.rfind(':').filter(|&i| !rest[i..].contains('/')) {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    let Some(rule) = rules.iter().find(|r| r.name == name) else {
        return image.to_string();
    };
    let name = rule.new_name.as_deref().unwrap_or(name);
    match (&rule.digest, &rule.new_tag) {
        (Some(digest), _) => format!("{name}@{digest}"),
        (None, Some(tag)) => format!("{name}:{tag}"),
        // Only a new name: the tag and the digest it had stay.
        (None, None) => {
            let tag = tag.map_or(String::new(), |t| format!(":{t}"));
            let digest = digest.map_or(String::new(), |d| format!("@{d}"));
            format!("{name}{tag}{digest}")
        }
    }
}

/// An image of an object once its overlay is evaluated, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Image {
    pub root: String,
    pub address: String,
    pub container: String,
    pub image: String,
}

/// An entry of a kustomization that is not followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unresolved {
    pub root: String,
    /// The kustomization file that names it.
    pub path: String,
    pub entry: String,
    /// `remote`, `outside` (above the repository or absolute), `missing` (no file or
    /// kustomization there, a symlink included), `cycle` or `depth`.
    pub reason: &'static str,
}

/// What the Kubernetes files of a repository give.
#[derive(Debug, Default)]
pub(crate) struct Output {
    pub nodes: Vec<Node>,
    pub references: Vec<Reference>,
    pub images: Vec<Image>,
    pub unresolved: Vec<Unresolved>,
}

/// Where an entry of a kustomization leads.
enum Entry<'m> {
    File(&'m str, &'m [Obj]),
    Dir(&'m str),
    Unresolved(&'static str),
}

struct Eval<'m> {
    files: &'m BTreeMap<String, Parsed>,
    /// The directories that hold a kustomization.
    dirs: BTreeMap<&'m str, (&'m str, &'m Kustomization)>,
    root: String,
    /// The objects the overlay may still instantiate, and the entries it may still walk.
    room: usize,
    steps: usize,
    /// The files an overlay instantiated: no plain unit holds them.
    reached: HashSet<&'m str>,
    root_left: usize,
    left: usize,
    /// The composite and claim kinds the repository's XRDs define (#655).
    kinds: crossplane::Kinds,
    out: Output,
}

fn remote(entry: &str) -> bool {
    entry.contains("://")
        || entry.starts_with("git@")
        || entry.contains("?ref=")
        || entry.starts_with("github.com/")
        || entry.starts_with("gitlab.com/")
}

impl<'m> Eval<'m> {
    /// Whether `bytes` more fit both budgets, taking them when they do.
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

    /// Where `entry` of the kustomization in `dir` leads.
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

    /// The objects of the kustomization in `dir`, the bases' first, once it has changed them.
    /// `stack` holds the directories above it.
    fn objects(&mut self, dir: &'m str, depth: usize, stack: &mut Vec<&'m str>) -> Vec<Obj> {
        let (path, k) = self.dirs[dir];
        let mut objs: Vec<Obj> = Vec::new();
        for entry in &k.entries {
            if self.steps == 0 {
                break;
            }
            self.steps -= 1;
            let found = match self.entry(dir, entry) {
                Entry::File(file, objects) => {
                    self.reached.insert(file);
                    let take = objects.len().min(self.room);
                    self.room -= take;
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
                    objs.extend(self.objects(base, depth + 1, stack));
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
        for o in &mut objs {
            if !(k.prefix.is_empty() && k.suffix.is_empty()
                || UNPREFIXED.contains(&o.kind.as_str()))
            {
                o.name = format!("{}{}{}", k.prefix, o.name, k.suffix);
            }
            if let Some(ns) = &k.namespace
                && !cluster_scoped(&o.kind, &o.group)
            {
                o.ns = Some(ns.clone());
            }
            for (_, image) in &mut o.facts.images {
                *image = retag(image, &k.images);
            }
        }
        objs
    }

    /// The nodes, references and images of the unit `root` holds `objs` of.
    fn unit(&mut self, mut objs: Vec<Obj>) {
        objs.truncate(OBJECTS_MAX);
        objs.sort_by_cached_key(Obj::address);
        objs.dedup_by_key(|o| o.address());
        let addresses: Vec<String> = objs.iter().map(Obj::address).collect();
        let roles: Vec<Option<crossplane::Role>> =
            objs.iter().map(|o| self.kinds.role(o)).collect();
        let mut kept = 0;
        for (o, address) in objs.iter().zip(&addresses) {
            if !self.charge(address.len() + o.path.len()) {
                break;
            }
            kept += 1;
            self.out.nodes.push(Node {
                address: address.clone(),
                root: self.root.clone(),
                ty: o.kind.clone(),
                // A managed resource's name in the provider's API is its external name.
                name: Some(
                    o.facts
                        .xp
                        .external
                        .clone()
                        .unwrap_or_else(|| o.name.clone()),
                ),
                pattern: false,
                overridable: Vec::new(),
                path: o.path.clone(),
                line: o.line,
                end_line: o.end_line,
                block: o.block.clone(),
                image: None,
                build: None,
                role: roles[kept - 1].map(|r| r.text().into()),
            });
        }
        let (objs, addresses, roles) = (&objs[..kept], &addresses[..kept], &roles[..kept]);
        // The resources each composition creates, under its address: its index, the address, and
        // the resource.
        let mut composed: Vec<(usize, String, &crossplane::Base)> = Vec::new();
        // The entries of `composed` each composition holds, by its index.
        let mut held: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (i, o) in objs.iter().enumerate() {
            for b in &o.facts.xp.bases {
                let address = format!("{}/{}", addresses[i], b.resource);
                if !seen.insert(address.clone()) || !self.charge(address.len() + o.path.len()) {
                    continue;
                }
                self.out.nodes.push(Node {
                    address: address.clone(),
                    root: self.root.clone(),
                    ty: b.kind.1.clone(),
                    name: Some(b.name.clone().unwrap_or_else(|| "*".into())),
                    pattern: b.name.is_none(),
                    overridable: Vec::new(),
                    path: o.path.clone(),
                    line: b.line,
                    end_line: b.end_line,
                    block: b.block.clone(),
                    image: None,
                    build: None,
                    role: Some(crossplane::Role::Composed.text().into()),
                });
                held.entry(i).or_default().push(composed.len());
                composed.push((i, address, b));
            }
        }
        let crossplane = crossplane::Targets::new(objs);
        // What a reference names: kind, name as written, namespace.
        let mut named: HashMap<(&str, &str, Option<&str>), Vec<usize>> = HashMap::new();
        // The pod template owners that carry a label, by namespace.
        let mut labelled: HashMap<(Option<&str>, &str, &str), Vec<usize>> = HashMap::new();
        for (i, o) in objs.iter().enumerate() {
            if o.group.is_empty() {
                named
                    .entry((&o.kind, &o.orig, o.ns.as_deref()))
                    .or_default()
                    .push(i);
            }
            for (k, v) in o.facts.labels.iter().flatten() {
                labelled.entry((o.ns.as_deref(), k, v)).or_default().push(i);
            }
        }
        for (i, o) in objs.iter().enumerate() {
            let mut targets: BTreeSet<(&str, String)> = BTreeSet::new();
            let ns = o.ns.as_deref();
            let find = |kind: &str, name: &str| {
                named
                    .get(&(kind, name, ns))
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>()
            };
            if let Some((k, v)) = o.facts.selector.iter().next() {
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
                for j in find(kind, name) {
                    targets.insert((&addresses[j], via.clone()));
                }
            }
            for j in o
                .facts
                .account
                .iter()
                .flat_map(|a| find("ServiceAccount", a))
            {
                targets.insert((&addresses[j], "serviceAccountName".into()));
            }
            for (name, via) in &o.facts.backends {
                for j in find("Service", name) {
                    targets.insert((&addresses[j], via.clone()));
                }
            }
            let mut found = BTreeSet::new();
            crossplane.of(o, roles[i], &mut found);
            for (j, via) in found {
                targets.insert((&addresses[j], via));
            }
            // Each resource a composition creates is one reference past the ceiling's.
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
            for (j, via) in crossplane
                .of_base(b)
                .into_iter()
                .take(OBJECT_REFERENCES_MAX)
            {
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
    }

    fn start(&mut self, label: &str) {
        self.root = label.to_string();
        self.room = OBJECTS_MAX;
        self.steps = WALK_MAX;
        self.root_left = UNIT_STORED_MAX;
    }
}

/// The IaC nodes, references, images and unresolved entries of the YAML and JSON files `files`
/// (path and bytes; empty bytes for a file that exists and cannot hold an object) of one
/// repository, each overlay and each directory of plain manifests evaluated alone, in label order.
pub(crate) fn evaluate(files: &[(String, Vec<u8>)]) -> Output {
    let parsed: BTreeMap<String, Parsed> = (files.iter())
        .map(|(path, src)| (path.clone(), read(path, src)))
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
        steps: 0,
        reached: HashSet::new(),
        root_left: 0,
        left: KUBE_STORED_MAX,
        kinds: crossplane::Kinds::of(parsed.values().flat_map(|p| &p.objects)),
        out: Output::default(),
    };
    // The directories that are bases, and the files that patch, which no plain unit holds.
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
        let objs = eval.objects(dir, 0, &mut vec![dir]);
        eval.unit(objs);
    }
    // A file only a kustomization nobody evaluates lists (a cycle of bases) is plain.
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

/// Whether a refresh that read `path` again (or lost it) must evaluate the objects again: a
/// kustomization, a file that may hold an object, or one that held a node.
pub(crate) fn changed(tx: &Transaction, path: &str, src: Option<&[u8]>) -> Result<bool> {
    if !is_source(path) {
        return Ok(false);
    }
    // `wanted` holds for a kustomization too, which a file that left is known by its name.
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

/// Replaces every stored Kubernetes node, reference, image and unresolved entry with `output`'s;
/// the Terraform rows stay.
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

/// The images of the node `address` of `root`, one `<container>: <image>` line each, in the
/// order of the pod spec.
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

/// The `meada status` line of the kustomization entries not followed; `None` with none.
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
        http:\n        paths:\n          - path: /api\n            backend:\n              \
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
        // A stream of several documents starts its key paths with the document's index.
        let secret = (out.nodes.iter()).find(|n| n.ty == "Secret").unwrap();
        assert_eq!(secret.block, "[2]");
    }

    #[test]
    fn selectors_mounts_accounts_and_backends_are_references_within_a_namespace() {
        let out = evaluate(&files(&[
            ("k/deploy.yaml", DEPLOYMENT),
            ("k/rest.yaml", REST),
            // The same names in another namespace are no targets.
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
        // The base's images rule runs first, the overlay's on its result.
        assert_eq!(
            (out.images.iter())
                .map(|i| i.image.as_str())
                .collect::<Vec<_>>(),
            ["registry.example.com/api:2.0"]
        );
        // The reference matches the name as written, before any prefix.
        assert_eq!(
            shown(&out),
            [
                "Deployment.apps/dev/dev-b-api-x -> ConfigMap/dev/dev-b-settings-x via envFrom",
                "Deployment.apps/dev/dev-b-api-x -> ConfigMap/dev/dev-b-settings-x via volume conf"
            ]
        );
    }

    #[test]
    fn an_image_is_retagged_by_the_first_rule_that_names_it() {
        let rule = |name: &str, new_name: Option<&str>, tag: Option<&str>, digest: Option<&str>| {
            ImageRule {
                name: name.into(),
                new_name: new_name.map(Into::into),
                new_tag: tag.map(Into::into),
                digest: digest.map(Into::into),
            }
        };
        let rules = [
            rule("nginx", None, Some("1.25"), None),
            rule("reg:5000/app", Some("other/app"), None, None),
            rule("db", None, Some("9"), Some("sha256:abc")),
        ];
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
                 - ../../../etc\n  - /abs\n  - gone.yaml\n  - ../a/b/../../o/self.yaml\n",
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
                ("../../../etc", "outside"),
                ("/abs", "outside"),
                ("gone.yaml", "missing"),
            ]
        );
        // A `..` that stays inside the repository is followed.
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
            // Naming each other only: no root at all.
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
        // The only file the cycle of x and y lists is no one's, so it is a plain object.
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
        // What the walk never reached is plain, in the unit of its directory.
        let overlay = out.nodes.iter().filter(|n| n.root == "d0");
        assert_eq!(overlay.count(), OVERLAY_DEPTH_MAX);
        let past: Vec<_> = out.unresolved.iter().map(|u| u.reason).collect();
        assert_eq!(past, ["depth"]);
    }

    /// Each base names the next three, a dozen levels deep: 3^12 paths an unbounded walk takes.
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
        // The walk stopped: the entries past the ceiling were never looked at.
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
        // One file holds at most `DOCUMENTS_MAX` documents; several give the count.
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
        // Many overlays over one base whose objects have long names.
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
