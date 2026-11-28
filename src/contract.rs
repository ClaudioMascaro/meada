use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use crate::alert::Alert;
use crate::channel::Channel;
use crate::ci::Ci;
use crate::entry::EntryKind;
use crate::http::Http;
use crate::image::Image;
use crate::index::{Consumer, Export, Indexed, Provider};
use crate::package::Packages;
use crate::resolve::Code;
use crate::resource::Resource;
use crate::shape::Shape;
use crate::workspace::Repository;

pub use meada_core::export::{Level, Link};

pub(crate) mod azure;
pub(crate) mod gcp;
mod receiver;
pub(crate) mod sdk;
pub use receiver::{EnvironmentScope, EnvironmentValue, ReceiverIdentity};

pub struct ProvisionUse {
    pub site: String,
    pub contract: String,
    pub level: Level,
    pub reason: String,
}

pub struct Found {
    pub role: Role,
    pub site: Site,
    pub contract: Option<String>,
    pub via: Option<String>,
    pub method: Option<String>,
    pub fragments: Vec<String>,
    pub shape: Option<Shape>,
    pub identity: Option<ReceiverIdentity>,
    pub lead: Option<Lead>,
    pub cap: Option<Level>,
    pub client: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Lead {
    pub limit: crate::lang::Limit,
    pub path: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Provider,
    Consumer,
    Binding,
    Publish,
    Import,
    Shape,
    Unmounted,
    Unfollowed,
}

impl Role {
    #[cfg(test)]
    pub const ALL: [Role; 8] = [
        Role::Provider,
        Role::Consumer,
        Role::Binding,
        Role::Publish,
        Role::Import,
        Role::Shape,
        Role::Unmounted,
        Role::Unfollowed,
    ];

    /// The name the index stores.
    pub fn name(self) -> &'static str {
        match self {
            Role::Provider => "provider",
            Role::Consumer => "consumer",
            Role::Binding => "binding",
            Role::Publish => "publish",
            Role::Import => "import",
            Role::Shape => "shape",
            Role::Unmounted => "unmounted",
            Role::Unfollowed => "unfollowed",
        }
    }
}

pub enum Site {
    Def {
        path: String,
        name: String,
    },
    File(String),
    Inline {
        path: String,
        def: String,
        name: String,
    },
    Call {
        path: String,
        index: usize,
    },
    Reference {
        path: String,
        index: usize,
    },
}

impl Site {
    pub fn path(&self) -> &str {
        match self {
            Site::Def { path, .. }
            | Site::Inline { path, .. }
            | Site::File(path)
            | Site::Call { path, .. }
            | Site::Reference { path, .. } => path,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Search {
    Filter(&'static str),
    Own(&'static str),
}

impl Search {
    pub fn word(self) -> &'static str {
        match self {
            Search::Filter(word) | Search::Own(word) => word,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct View {
    pub provider: &'static str,
    pub consumer: Option<&'static str>,
    pub carried: &'static str,
}

impl View {
    pub fn follows_drift(&self) -> bool {
        self.consumer.is_none()
    }
}

pub struct Related<'e> {
    pub site: &'e str,
    pub role: Role,
    pub words: String,
    pub matched: Option<(Level, String)>,
    pub shape: Option<&'e Shape>,
}

pub use meada_core::search::Hit;

#[derive(Default)]
pub struct ConfigFacts<'a> {
    pub(crate) nodes: &'a [&'a crate::iac::Node],
    pub(crate) helm_env: &'a [String],
}

pub trait ContractKind {
    fn prefix(&self) -> &'static str;

    fn extract(&self, code: &Code, indexed: &Indexed) -> Vec<Found>;

    fn environment_name(
        &self,
        contract: &str,
        value: &str,
        _operation: Option<&str>,
    ) -> Option<String> {
        environment_name(contract, value)
    }

    fn environment_target(&self, contract: &str, target: &str, _operation: Option<&str>) -> bool {
        target
            .rsplit_once("::")
            .is_some_and(|(prefix, _)| prefix == contract)
    }

    fn environment_identity(
        &self,
        _contract: &str,
        _identity: &ReceiverIdentity,
        _values: &dyn Fn(&str) -> Vec<EnvironmentValue>,
    ) -> Option<Vec<(String, String)>> {
        None
    }

    fn config_file(&self, _path: &str) -> bool {
        false
    }

    fn config_summary(
        &self,
        _path: &str,
        _src: &[u8],
        _root: &std::path::Path,
        _facts: &ConfigFacts<'_>,
    ) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn executions(
        &self,
        _exports: &[Export],
        _links: &[Link],
    ) -> Vec<meada_core::export::Execution> {
        Vec::new()
    }

    fn entry(&self, _role: Role, _contract: &str) -> Option<EntryKind> {
        None
    }

    fn declare(&self, _repo: &Repository, _export: &mut Export) {}

    fn read_declarations(
        &self,
        _repo: &Repository,
        _db: &rusqlite::Connection,
        _export: &mut Export,
    ) -> crate::Result<()> {
        Ok(())
    }

    fn symbols_read(&self, _repo: &Repository, _export: &Export) -> Option<Vec<String>> {
        Some(Vec::new())
    }

    fn workspace_step(&self, _exports: &mut [Export], _repositories: &[Repository]) {}

    fn workspace_ready(&self, _exports: &mut [Export], _repositories: &[Repository]) {}

    fn provision_uses(&self, _exports: &[Export]) -> Vec<(usize, ProvisionUse)> {
        Vec::new()
    }

    fn link(
        &self,
        consumer: &Consumer,
        from: &Repository,
        providers: &[(&Repository, &Provider)],
        exports: &[Export],
    ) -> Vec<Link>;

    fn workspace_links(
        &self,
        _exports: &[Export],
        _providers: &[(&Repository, &Provider)],
    ) -> Option<Vec<Link>> {
        None
    }

    fn after_links(&self, _links: &mut [Link], _exports: &[Export]) {}

    fn calls_from_definition(&self) -> bool {
        false
    }

    fn starts_at_provider(&self) -> bool {
        false
    }

    fn flows_to_consumer(&self) -> bool {
        false
    }

    fn exports_repository_links(&self) -> bool {
        false
    }

    fn unresolved(&self, _contract: &str) -> bool {
        false
    }

    fn use_level(&self, _via: Option<&str>, _operation: Option<&str>) -> Level {
        Level::Exact
    }

    fn label(&self, contract: &str) -> String {
        shown(contract)
    }

    fn parts<'c>(&self, contract: &'c str) -> (Option<&'c str>, Option<&'c str>) {
        (None, last(contract))
    }

    fn provisioned_group<'c>(&self, _contract: &'c str) -> Option<&'c str> {
        None
    }

    fn place(&self, _repository: &str, _site: &str) -> Option<String> {
        None
    }

    fn search_word(&self) -> Search {
        Search::Filter(self.prefix())
    }

    fn own_search(&self, _exports: &[Export], _links: &[Link], _needle: &str) -> Vec<Hit> {
        Vec::new()
    }

    fn in_default_search(&self) -> bool {
        true
    }

    fn reads_commit(&self) -> bool {
        false
    }

    fn written(&self, _nodes: &[crate::iac::Node], _out: &mut meada_core::export::Export) {}

    fn sides(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    fn view(&self, _contract: &str) -> Option<View> {
        self.sides().map(|(provider, consumer)| View {
            provider,
            consumer: Some(consumer),
            carried: "payload",
        })
    }

    fn related_sides<'e>(
        &self,
        _exports: &'e [Export],
        _export: &'e Export,
        _contract: &str,
    ) -> Vec<Related<'e>> {
        Vec::new()
    }

    fn workspace_related_sides<'e>(
        &self,
        _exports: &'e [Export],
        _links: &[Link],
        _contract: &str,
    ) -> Option<Vec<Vec<Related<'e>>>> {
        None
    }

    fn at_call(&self, _exports: &[Export], _export: &Export, _call: &str) -> Vec<String> {
        Vec::new()
    }

    fn explain_inferred(
        &self,
        _consumer: &Consumer,
        _from: &Repository,
        _link: &Link,
        _exports: &[Export],
    ) -> bool {
        false
    }

    fn context_sections(
        &self,
        _exports: &[Export],
        _repos: &[Repository],
        _links: &[Link],
        _symbol: &str,
    ) -> Vec<String> {
        Vec::new()
    }

    fn context_at(
        &self,
        exports: &[Export],
        repos: &[Repository],
        links: &[Link],
        site: &str,
        _root: Option<&str>,
    ) -> Vec<String> {
        self.context_sections(exports, repos, links, site)
    }

    fn context_reads(&self, _links: &[Link], _symbol: &str) -> Vec<String> {
        Vec::new()
    }

    fn starts<'a>(&self, _from: &str, _providers: &[(&'a str, &'a str)]) -> Vec<&'a str> {
        Vec::new()
    }

    fn status(&self, _repository: &Repository, _export: &Export, _links: &[Link]) -> Vec<String> {
        Vec::new()
    }

    fn scope_status(
        &self,
        repository: &Repository,
        export: &Export,
        _exports: &[Export],
        links: &[Link],
    ) -> Vec<String> {
        self.status(repository, export, links)
    }

    fn workspace_status(
        &self,
        _repositories: &[Repository],
        _exports: &[Export],
        _links: &[Link],
    ) -> Option<Vec<Vec<String>>> {
        None
    }

    fn side_note(&self, _exports: &[Export], _contract: &str, _role: Role) -> &'static str {
        ""
    }

    fn decidable(&self) -> bool {
        true
    }

    fn consumer_item(&self, _link: &Link, _exports: &[Export]) -> bool {
        self.decidable()
    }

    fn owns_decision(&self, _decision: &crate::decisions::Decision) -> bool {
        false
    }

    fn decision_items(&self, _exports: &[Export]) -> Vec<(crate::items::OpenItem, bool)> {
        Vec::new()
    }

    fn decision_reports(
        &self,
        _workspace: &crate::workspace::Workspace,
        _exports: &[Export],
    ) -> Vec<crate::decisions::Evaluation> {
        Vec::new()
    }

    fn serves(&self, provider: &Provider, contract: &str) -> bool {
        provider.contract == contract
    }

    fn link_candidate(&self, provider: &Provider, link: &Link, _exports: &[Export]) -> bool {
        provider.site == link.provider && serves(provider, &link.contract)
    }

    fn unresolved_causes(&self, _exports: &[Export]) -> HashMap<String, &'static str> {
        HashMap::new()
    }

    fn no_candidates(
        &self,
        _consumer: &Consumer,
        _providers: &mut dyn Iterator<Item = &Provider>,
        _missing: &str,
    ) -> Option<String> {
        None
    }

    fn candidate(&self, _consumer: &Consumer, _provider: &Provider) -> bool {
        false
    }
}

pub const KINDS: &[&dyn ContractKind] =
    &[&Http, &Resource, &Channel, &Packages, &Image, &Alert, &Ci];

pub(crate) fn workspace_steps(exports: &mut [Export], repositories: &[Repository]) {
    for export in exports.iter_mut() {
        export.provision_uses.clear();
    }
    for kind in KINDS {
        kind.workspace_step(exports, repositories);
    }
    for kind in KINDS {
        kind.workspace_ready(exports, repositories);
    }
    for kind in KINDS {
        for (repository, side) in kind.provision_uses(exports) {
            if let Some(export) = exports.get_mut(repository) {
                export.provision_uses.push(side);
            }
        }
    }
}

pub(crate) fn config_file(path: &str) -> bool {
    KINDS.iter().any(|k| k.config_file(path))
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub(crate) enum Probe {
    Missing,
    Link,
    Dir,
    File,
    Other,
}

#[derive(Default)]
pub(crate) struct Seen {
    pub probes: std::collections::BTreeMap<String, Probe>,
    pub reads: std::collections::BTreeMap<String, String>,
}

thread_local! {
    static SEEN: std::cell::RefCell<Option<Seen>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn probe(root: &std::path::Path, rel: &str) -> Probe {
    let found = match std::fs::symlink_metadata(root.join(rel)) {
        Err(_) => Probe::Missing,
        Ok(m) if m.file_type().is_symlink() => Probe::Link,
        Ok(m) if m.is_dir() => Probe::Dir,
        Ok(m) if m.is_file() => Probe::File,
        Ok(_) => Probe::Other,
    };
    SEEN.with_borrow_mut(|seen| {
        if let Some(seen) = seen {
            seen.probes.insert(rel.to_string(), found);
        }
    });
    found
}

// ponytail: a listing of 4,096 entries names a directory a wildcard source reads; a longer
// directory is not listed (`list` gives `None`, its fingerprint reads `unread`), so the summary
// that wanted it falls back before it depends on it.
pub(crate) const LIST_MAX: usize = 4096;

fn load(root: &std::path::Path, rel: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let path = root.join(rel);
    if !std::fs::symlink_metadata(&path).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(crate::lang::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= crate::lang::MAX_BYTES).then_some(bytes)
}

fn entries(root: &std::path::Path, rel: &str) -> Option<Vec<(String, Probe)>> {
    let path = root.join(rel);
    if !std::fs::symlink_metadata(&path).ok()?.is_dir() {
        return None;
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        let kind = match entry.file_type().ok()? {
            t if t.is_symlink() => Probe::Link,
            t if t.is_dir() => Probe::Dir,
            t if t.is_file() => Probe::File,
            _ => Probe::Other,
        };
        out.push((entry.file_name().into_string().ok()?, kind));
        if out.len() > LIST_MAX {
            return None;
        }
    }
    out.sort();
    Some(out)
}

pub(crate) fn fingerprint(root: &std::path::Path, key: &str) -> String {
    let hash = match key.strip_suffix('/') {
        Some(dir) => entries(root, dir).map(|entries| {
            let mut hasher = blake3::Hasher::new();
            for (name, kind) in entries {
                hasher.update(format!("{name}\0{}\n", kind as u8).as_bytes());
            }
            hasher.finalize()
        }),
        None => load(root, key).map(|bytes| blake3::hash(&bytes)),
    };
    hash.map_or_else(|| "unread".into(), |hash| hash.to_hex().to_string())
}

pub(crate) fn read(root: &std::path::Path, rel: &str) -> Option<Vec<u8>> {
    let bytes = load(root, rel);
    SEEN.with_borrow_mut(|seen| {
        if let Some(seen) = seen {
            seen.reads.insert(rel.to_string(), fingerprint(root, rel));
        }
    });
    bytes
}

pub(crate) fn list(root: &std::path::Path, rel: &str) -> Option<Vec<(String, Probe)>> {
    let listed = entries(root, rel);
    SEEN.with_borrow_mut(|seen| {
        if let Some(seen) = seen {
            seen.reads
                .insert(format!("{rel}/"), fingerprint(root, &format!("{rel}/")));
        }
    });
    listed
}

pub(crate) fn probed<T>(summarize: impl FnOnce() -> T) -> (T, Seen) {
    let outer = SEEN.replace(Some(Default::default()));
    let value = summarize();
    (value, SEEN.replace(outer).unwrap_or_default())
}

pub(crate) fn config_summary(
    path: &str,
    src: &[u8],
    root: &std::path::Path,
    nodes: &[&crate::iac::Node],
    helm_env: &[String],
) -> serde_json::Value {
    let mut summaries = serde_json::Map::new();
    let facts = ConfigFacts { nodes, helm_env };
    for kind in KINDS.iter().filter(|k| k.config_file(path)) {
        summaries.insert(
            kind.prefix().into(),
            kind.config_summary(path, src, root, &facts),
        );
    }
    summaries.into()
}

pub fn kind_of(contract: &str) -> Option<&'static dyn ContractKind> {
    let (prefix, _) = contract.split_once("::")?;
    KINDS.iter().copied().find(|k| k.prefix() == prefix)
}

pub(crate) fn environment_name(prefix: &str, value: &str) -> Option<String> {
    if !(prefix.starts_with("channel::") || prefix.starts_with("resource::")) {
        return None;
    }
    if let Some(contract) = crate::iac::types::identified(value) {
        return (contract.rsplit_once("::")?.0 == prefix).then_some(contract);
    }
    (!value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || b"._-".contains(&b)
                || (prefix.starts_with("resource::") && b == b'/')
        }))
    .then(|| format!("{prefix}::{value}"))
}

pub(crate) fn environment_reason(via: Option<&str>) -> Option<&str> {
    via.filter(|v| v.starts_with("workload:"))
}

pub(crate) fn environment_cap(cap: Option<Level>, via: Option<&str>) -> Option<Level> {
    cap.or_else(|| environment_reason(via).map(|_| Level::Inferred))
}

pub(crate) fn environment_link(link: &mut Link, cap: Option<Level>, via: Option<&str>) {
    if let Some(level) = environment_cap(cap, via) {
        link.level = link.level.max(level);
    }
    if let Some(reason) = environment_reason(via) {
        let evidence = format!("; {reason}");
        if !link.reason.contains(&evidence) {
            link.reason += &evidence;
        }
    }
}

pub(crate) fn use_level(
    contract: &str,
    cap: Option<Level>,
    via: Option<&str>,
    operation: Option<&str>,
) -> Level {
    let level = kind_of(contract).map_or(Level::Declared, |kind| kind.use_level(via, operation));
    environment_cap(cap, via).map_or(level, |cap| level.max(cap))
}

pub(crate) fn link_in_repository(link: &Link, repository: &str) -> bool {
    let prefix = format!("{repository}::");
    link.consumer.starts_with(&prefix)
        || link.provider.starts_with(&prefix)
        || (kind_of(&link.contract).is_some_and(|k| k.exports_repository_links())
            && (link.consumer == repository || link.provider == repository))
}

pub fn calls_from_definition(contract: Option<&str>) -> bool {
    contract.is_none_or(|c| kind_of(c).is_some_and(|k| k.calls_from_definition()))
}

pub fn flows_to_consumer(contract: Option<&str>) -> bool {
    contract
        .and_then(kind_of)
        .is_some_and(|kind| kind.flows_to_consumer())
}

pub(crate) fn explain_inferred(
    consumer: &Consumer,
    from: &Repository,
    link: &Link,
    exports: &[Export],
) -> bool {
    consumer
        .contract
        .as_deref()
        .and_then(kind_of)
        .is_some_and(|kind| kind.explain_inferred(consumer, from, link, exports))
}

pub fn unresolved(contract: &str) -> bool {
    kind_of(contract).is_some_and(|k| k.unresolved(contract))
}

pub fn shown(contract: &str) -> String {
    match unresolved(contract) {
        true => format!("{contract} (unresolved)"),
        false => contract.to_string(),
    }
}

pub fn typed(text: &str) -> &str {
    match text.strip_suffix(" (unresolved)") {
        Some(contract) if unresolved(contract) => contract,
        _ => text,
    }
}

pub fn label(contract: &str) -> String {
    kind_of(contract).map_or_else(|| shown(contract), |k| k.label(contract))
}

pub fn searched(word: &str) -> Option<(&'static dyn ContractKind, Search)> {
    let words = KINDS.iter().map(|k| (*k, k.search_word()));
    words.into_iter().find(|(_, search)| search.word() == word)
}

fn last(contract: &str) -> Option<&str> {
    contract.rsplit_once("::").map(|(_, part)| part)
}

pub fn parts(contract: &str) -> (Option<&str>, Option<&str>) {
    kind_of(contract).map_or_else(|| (None, last(contract)), |k| k.parts(contract))
}

pub fn place(contract: &str, repository: &str, site: &str) -> Option<String> {
    kind_of(contract)?.place(repository, site)
}

pub fn serves(provider: &Provider, contract: &str) -> bool {
    kind_of(contract).map_or(provider.contract == contract, |k| {
        k.serves(provider, contract)
    })
}

pub(crate) fn consumer_item(link: &Link, exports: &[Export]) -> bool {
    link.level == Level::Ambiguous
        && kind_of(&link.contract).is_none_or(|kind| kind.consumer_item(link, exports))
}

pub(crate) fn link_candidate(provider: &Provider, link: &Link, exports: &[Export]) -> bool {
    kind_of(&link.contract).map_or(
        provider.site == link.provider && provider.contract == link.contract,
        |kind| kind.link_candidate(provider, link, exports),
    )
}

pub fn entry(role: Role, contract: &str) -> Option<EntryKind> {
    kind_of(contract)?.entry(role, contract)
}

#[derive(Default)]
pub struct Names {
    named: HashMap<(String, String), String>,
    unknown: HashSet<String>,
    missed: Cell<bool>,
}

impl Names {
    pub fn new(found: &[Found]) -> Self {
        let mut proposed: Vec<(&str, usize, &str, &str)> = Vec::new();
        for f in found {
            if let Site::Inline { path, def, name } = &f.site {
                let segment = def.rsplit('.').next().unwrap_or(def);
                let offset = segment
                    .trim_start_matches('@')
                    .parse()
                    .unwrap_or(usize::MAX);
                proposed.push((path, offset, def, name));
            }
        }
        proposed.sort_by_key(|&(path, offset, _, _)| (path, offset));
        proposed.dedup_by_key(|&mut (path, offset, _, _)| (path, offset));
        let mut names = Self::default();
        let mut taken: HashMap<(String, String, String), usize> = HashMap::new();
        for (path, _, def, name) in proposed {
            let (parent, segment) = def.rsplit_once('.').unwrap_or(("", def));
            let parent = names.scope(path, parent);
            let n = taken.entry((path.into(), parent, name.into())).or_insert(0);
            let named = match *n {
                0 => name.to_string(),
                n => format!("{name}#{n}"),
            };
            *n += 1;
            names.named.insert((path.into(), segment.into()), named);
        }
        names
    }

    pub fn without(mut self, paths: HashSet<String>) -> Self {
        self.unknown = paths;
        self
    }

    pub fn missed(&self) -> bool {
        self.missed.get()
    }

    fn note(&self, path: &str, name: &str) {
        if name.contains('@') && self.unknown.contains(path) {
            self.missed.set(true);
        }
    }

    pub fn def(&self, path: &str, name: &str) -> Option<String> {
        self.note(path, name);
        let last = name.rsplit('.').next().unwrap_or(name);
        if last.starts_with('@') && !self.named.contains_key(&(path.into(), last.into())) {
            return None;
        }
        Some(self.scope(path, name))
    }

    pub fn scope(&self, path: &str, scope: &str) -> String {
        self.note(path, scope);
        if !scope.contains('@') {
            return scope.to_string();
        }
        scope
            .split('.')
            .filter_map(|segment| match segment.starts_with('@') {
                true => self.named.get(&(path.into(), segment.into())).cloned(),
                false => Some(segment.to_string()),
            })
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Channel, ContractKind, Http, KINDS, Level, Link, Packages, Resource, Search, View, place,
        searched, shown, typed, unresolved,
    };
    use crate::config::Config;
    use crate::decisions::Decisions;
    use crate::entry::{EntryKind, EntryPoint};
    use crate::index::Export;
    use crate::workspace::Repository;

    #[test]
    fn shown_and_typed_round_trip_through_the_kind() {
        let broker_only = "channel::kafka";
        assert!(unresolved(broker_only));
        assert_eq!(shown(broker_only), "channel::kafka (unresolved)");
        assert_eq!(typed("channel::kafka (unresolved)"), broker_only);

        let named = "channel::kafka::orders.created";
        assert!(!unresolved(named));
        assert_eq!(shown(named), named);
        assert_eq!(typed(named), named);

        assert_eq!(
            typed("channel::kafka::x (unresolved)"),
            "channel::kafka::x (unresolved)"
        );
        assert_eq!(shown("http::GET::/users"), "http::GET::/users");
        assert_eq!(shown("not a contract"), "not a contract");
    }

    fn resource_repo(toml: &str) -> Repository {
        Repository {
            root: "".into(),
            identity: "r".into(),
            config: toml::from_str::<Config>(toml).unwrap(),
            decisions: Decisions::default(),
            index: "".into(),
        }
    }

    fn entry(identity: &str, kind: EntryKind) -> EntryPoint {
        EntryPoint {
            identity: identity.into(),
            kind,
            label: identity.into(),
            site: identity.into(),
        }
    }

    #[test]
    fn resources_report_status_lines_and_other_kinds_none() {
        let repo =
            resource_repo("[[resource]]\nkind = \"postgres\"\nid = \"orders\"\nrole = \"read\"\n");
        let export = Export::default();
        let lines = Resource.status(&repo, &export, &[]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("resource::postgres::orders"), "{lines:?}");
        assert!(Http.status(&repo, &export, &[]).is_empty());
        assert!(Channel.status(&repo, &export, &[]).is_empty());
    }

    #[test]
    fn workspace_steps_keep_the_entry_points_of_a_workspace_without_imports() {
        let mut export = Export {
            repository: "r".into(),
            ..Export::default()
        };
        export.entry_points = vec![
            entry("r::a.ts::free", EntryKind::Export),
            entry("r::a.ts::route", EntryKind::Route),
        ];
        let mut exports = vec![export];
        for kind in [&Packages as &dyn ContractKind, &Http, &Resource, &Channel] {
            kind.workspace_step(&mut exports, &[]);
        }
        let kept: Vec<_> = (exports[0].entry_points.iter())
            .map(|e| &e.identity[..])
            .collect();
        assert_eq!(kept, ["r::a.ts::free", "r::a.ts::route"]);
    }

    #[test]
    fn the_kinds_search_words_follow_the_kinds_order() {
        let words: Vec<_> = KINDS.iter().map(|k| k.search_word()).collect();
        assert_eq!(
            words,
            [
                Search::Filter("route"),
                Search::Filter("resource"),
                Search::Filter("channel"),
                Search::Own("package"),
                Search::Filter("image"),
                Search::Filter("alert"),
                Search::Own("ci"),
            ]
        );
        let (kind, search) = searched("package").unwrap();
        assert_eq!((kind.prefix(), search), ("package", Search::Own("package")));
        assert!(searched("http").is_none());
    }

    #[test]
    fn a_kind_with_sides_is_viewed_by_both_and_one_without_has_no_view() {
        let channel = Channel.view("channel::kafka::orders.created").unwrap();
        let expected = View {
            provider: "producer",
            consumer: Some("consumer"),
            carried: "payload",
        };
        assert_eq!(channel, expected);
        let resource = Resource.view("resource::postgres::orders").unwrap();
        assert_eq!(
            (resource.provider, resource.consumer),
            ("writer", Some("reader"))
        );
        assert_eq!(
            Packages.view("package::npm::kit").unwrap().provider,
            "package"
        );
    }

    #[test]
    fn a_kind_with_nothing_to_add_leaves_the_sides_the_call_and_the_symbol_alone() {
        let exports = [Export::default()];
        for kind in [&Http as &dyn ContractKind, &Resource, &Packages] {
            let related = kind.related_sides(&exports, &exports[0], "x::y");
            assert!(related.is_empty());
            assert!(
                kind.at_call(&exports, &exports[0], "r::a.ts::f#0")
                    .is_empty()
            );
        }
        for kind in [&Http as &dyn ContractKind, &Resource, &Channel] {
            assert!(
                kind.context_sections(&exports, &[], &[], "r::a.ts::f")
                    .is_empty()
            );
        }
    }

    #[test]
    fn the_seam_places_a_manifest_site_and_leaves_any_other_alone() {
        assert_eq!(
            place("package::npm::kit", "r", "r::kit/package.json"),
            Some("kit/package.json".into())
        );
        assert_eq!(place("http::GET::/", "r", "r::src/app.ts::main"), None);
    }

    #[test]
    fn the_pass_after_links_leaves_a_link_without_shapes_alone() {
        let mut links = vec![Link::new(
            "r::a.ts::c".into(),
            "s::b.ts::p".into(),
            "http::GET::/x".into(),
            Level::Exact,
            "x".into(),
            None,
            None,
            None,
        )];
        Http.after_links(&mut links, &[]);
        Channel.after_links(&mut links, &[]);
        assert!(links[0].drift.is_none());
    }

    #[test]
    fn names_without_a_file_mark_a_question_about_its_inline_definitions() {
        let inline = |path: &str| super::Found {
            identity: None,
            role: super::Role::Provider,
            site: super::Site::Inline {
                path: path.into(),
                def: "app.@40".into(),
                name: "GET /a".into(),
            },
            contract: None,
            via: None,
            method: None,
            fragments: Vec::new(),
            shape: None,
            lead: None,
            cap: None,
            client: None,
        };
        let names = super::Names::new(&[inline("a.ts")]).without(["b.ts".to_string()].into());
        assert_eq!(names.scope("a.ts", "app.@40").as_str(), "app.GET /a");
        assert_eq!(
            names.def("b.ts", "app.handler").as_deref(),
            Some("app.handler")
        );
        assert!(!names.missed());
        assert_eq!(names.def("b.ts", "app.@40"), None);
        assert!(names.missed());
    }
}
