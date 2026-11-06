//! Backstage configuration reads in the URLs of TypeScript consumers (#617). The pack writes
//! what looks like a read, `config.getString('a.b')` or `getOptionalString`, as a `Part::Read`;
//! this module decides. The key is looked up in the repository's `app-config*.yaml` files
//! (`app-config.yaml` first, then the others in path order, the first that holds the key
//! deciding), and a value that is exactly `${VAR}` makes the read the variable `VAR`
//! (`Part::Env`), which `base_url_env` then maps. A literal value, a key no file holds and a
//! read with a member left over stay unknown, as before.
//!
//! Each root is read as a tree first (`includes`): an object whose only key is `$include` is
//! replaced by the file (or the `#` key path of it) its value names, and one whose only key is
//! `$env` by `${VAR}` (#689); the tree is then flattened as Spring's YAML is
//! (`spring::flatten`). A root past `lang::MAX_BYTES` is dropped, and a partial is never a root.
//! The includes not followed are `Role::Unfollowed` rows of the extraction (`unfollowed`), which
//! `meada status` names.
//!
//! The same files hold Backstage's proxy table (#616): `proxy.endpoints['/<ep>'].target`, and, when
//! no file has a `proxy.endpoints` key (nor an include not followed there, at `proxy` or at the
//! root), the keys of `proxy` that start with `/` (the legacy shape `proxy['/<ep>'].target`,
//! which Backstage's proxy backend reads when `endpoints` does not exist, across the merged files,
//! #690). An endpoint written as a string is `{ target }` in either shape (`expand`), with the
//! default `pathRewrite`. A URL that starts with `/api/proxy/<ep>` (the proxy plugin's mount, also
//! what `getBaseUrl('proxy')` folds to) is a call to that target: a `${VAR}` target is the variable
//! `VAR`, an `http(s)://` target is that host (with its path), and the rest of the URL follows it.
//! A `pathRewrite` that only strips the endpoint (`'^/api/proxy/<ep>': ''`, or
//! `'^/api/proxy/<ep>/': '/'` for a URL with a path) is what the proxy does anyway. Any other
//! target (a template around the variable, a default, no `http`) and any other `pathRewrite`
//! (another replacement, several rules, a pattern with regex characters), `ignorePath`,
//! `prependPath: false` and an endpoint key the files give different values leave the URL as
//! written and the call unlinked, and `meada status` says so.
// ponytail: every `app-config*.yaml` of the repository serves every file of it, whatever the
// directory, and profiles are not merged: the first file holding the key wins. Scope the lookup
// by directory if a monorepo's apps keep different values under one key.

mod includes;

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) use includes::{DEPTH, INCLUDES, Reason, Unfollowed};

use crate::contract::{Found, Role, Site};
use crate::index::Indexed;
use crate::jvm::spring::{Properties, flatten};
use crate::lang::{ConfigInput, Part, Value};
use crate::resolve::Code;

/// The reads of one extraction; the configuration files are parsed the first time one asks.
pub(in crate::http) struct Config<'i> {
    files: &'i [(&'i str, &'i [u8])],
    /// The config inputs that are symbolic links (`Indexed::symlinks`).
    symlinks: &'i [&'i str],
    keys: OnceCell<Read>,
    endpoints: OnceCell<Vec<Endpoint>>,
}

/// What the `app-config*.yaml` files give, read once.
struct Read {
    /// The keys of every file, the first file holding a key giving its value.
    keys: Properties,
    /// The keys two files give different values.
    contested: BTreeSet<String>,
    /// The includes no root followed, each once, sorted.
    unfollowed: Vec<Unfollowed>,
    /// Whether any file has a `proxy.endpoints` that is not null, an empty object included, or an
    /// include not followed where it may hold one (`TABLE_PATHS`): Backstage reads that table
    /// whenever it exists, so an empty or unread one leaves no key to say so.
    endpoints_table: bool,
}

/// A proxy endpoint: its name, its target, and whether its `pathRewrite` strips only `<ep>/`
/// (so a URL with nothing after `<ep>` or a `?` is not stripped, and its target is unread).
type Endpoint = (String, Target, bool);

/// What a proxy endpoint's `target` is.
enum Target {
    /// `${VAR}`.
    Env(String),
    /// A literal `http://` or `https://` URL, without a trailing `/`.
    Url(String),
    /// Anything else.
    Unread,
}

/// A URL through a proxy endpoint: the endpoint, and the URL's arguments with the proxy's mount
/// and the endpoint replaced by the target (`None` when the target is not read).
pub(super) struct Proxied {
    pub(super) endpoint: String,
    pub(super) args: Option<Vec<Value>>,
}

/// Where Backstage's proxy plugin is mounted.
const PROXY: &str = "/api/proxy";
/// Where a value left unread may hold the proxy table: the root, `proxy` and `proxy.endpoints`.
// ponytail: an `$env` beside another key there is unread too (Backstage refuses the config), so
// it hides the legacy keys, but it is no include and `meada status` names nothing; name it as an
// unfollowed row if that shows up.
const TABLE_PATHS: &[&[&str]] = &[&[], &["proxy"], &["proxy", "endpoints"]];
/// The flattened keys of the proxy table's endpoints, and of its legacy shape.
const ENDPOINTS: &str = "proxy.endpoints.";
const LEGACY: &str = "proxy.";

impl<'i> Config<'i> {
    pub(in crate::http) fn new(indexed: &Indexed<'i>) -> Self {
        Self::of(indexed.configs, indexed.symlinks)
    }

    fn of(configs: &'i [(&'i str, &'i [u8])], symlinks: &'i [&'i str]) -> Self {
        Self {
            files: configs,
            symlinks,
            keys: OnceCell::new(),
            endpoints: OnceCell::new(),
        }
    }

    /// The arguments of a `fetch` with the reads in its URL decided.
    pub(super) fn decided(&self, args: &[Value]) -> Vec<Value> {
        let mut args = args.to_vec();
        if let Some(Value::Str(parts)) = args.first_mut() {
            for part in parts.iter_mut() {
                if let Part::Read { key, then, .. } = part {
                    *part = match then.is_empty().then(|| self.variable(key)).flatten() {
                        Some(var) => Part::Env(var),
                        None => Part::Hole,
                    };
                }
            }
        }
        args
    }

    /// The variable a key's value names, when the first file holding the key gives `${VAR}`.
    fn variable(&self, key: &str) -> Option<String> {
        env_var(self.keys().get(key)?)
    }

    /// The keys of every `app-config*.yaml`, parsed the first time they are asked for.
    fn keys(&self) -> &Properties {
        &self.read().keys
    }

    /// The keys of every `app-config*.yaml`, those two of them disagree on, and the includes
    /// not followed.
    fn read(&self) -> &Read {
        self.keys.get_or_init(|| {
            let mut files: Vec<_> = self
                .files
                .iter()
                .filter(|(p, _)| is_app_config(p))
                .collect();
            // `app-config.yaml` is read first and wins: which override a deployment loads is not known.
            files.sort_by_key(|(p, _)| (!p.ends_with("app-config.yaml"), *p));
            let by_path: BTreeMap<&str, &[u8]> = self.files.iter().copied().collect();
            // What the index gave for each path a root's includes name.
            let read = |path: &str| match by_path.get(path) {
                Some(src) => ConfigInput::Read((*src).into()),
                None if self.symlinks.contains(&path) => ConfigInput::Symlink,
                None => ConfigInput::Missing,
            };
            let mut keys = BTreeMap::new();
            let mut contested = BTreeSet::new();
            let mut unfollowed = BTreeSet::new();
            let mut endpoints_table = false;
            for (path, src) in files {
                // An include not followed at the root, `proxy` or `proxy.endpoints` may hold the
                // table, and Backstage reads (or refuses) whatever it gives: the legacy keys are
                // no endpoints.
                let grafted = includes::tree(path, src, &read, TABLE_PATHS);
                unfollowed.extend(grafted.unfollowed);
                endpoints_table |= grafted.watched_unread;
                let Some(mut tree) = grafted.tree else {
                    continue;
                };
                expand(&mut tree);
                endpoints_table |= !tree["proxy"]["endpoints"].is_null();
                for (k, v) in flatten(&tree) {
                    match keys.get(&k) {
                        Some(kept) if *kept != v => {
                            contested.insert(k);
                        }
                        Some(_) => {}
                        None => {
                            keys.insert(k, v);
                        }
                    }
                }
            }
            Read {
                keys,
                contested,
                unfollowed: unfollowed.into_iter().collect(),
                endpoints_table,
            }
        })
    }

    /// The includes no root followed, each once, sorted.
    fn unfollowed(&self) -> &[Unfollowed] {
        &self.read().unfollowed
    }

    /// `proxy.endpoints['/<ep>'].target`, longest endpoint first; with no `proxy.endpoints` key in
    /// any file, `proxy['/<ep>'].target` (Backstage's legacy shape).
    // ponytail: the flattened keys tell an endpoint `/a` with a `target` from a string endpoint
    // named `/a.target` only by the suffix; read the parsed tree if an endpoint is named so.
    fn endpoints(&self) -> &[Endpoint] {
        self.endpoints.get_or_init(|| {
            let prefix = self.table();
            let mut out: Vec<_> = (self.keys().iter())
                .filter_map(|(key, value)| {
                    let name = key.strip_prefix(prefix)?.strip_suffix(".target")?;
                    let raw = name;
                    let name = name.trim_end_matches('/');
                    // `|` is the separator of a consumer's twin readings (`keep_applicable`).
                    if !name.starts_with('/') || name.contains('|') {
                        return None;
                    }
                    let (strip, slash) = self.strips(prefix, raw, name);
                    let target = if strip { target(value) } else { Target::Unread };
                    Some((name.to_string(), target, slash))
                })
                .collect();
            out.sort_by_key(|(name, ..)| std::cmp::Reverse(name.len()));
            out
        })
    }

    /// The key prefix the endpoints are read under: `proxy.endpoints.` when any file has a key
    /// there or a table `Read::endpoints_table` counts, else the legacy `proxy.`.
    fn table(&self) -> &'static str {
        if self.read().endpoints_table {
            return ENDPOINTS;
        }
        let keys = self.keys();
        let endpoints = keys.range(ENDPOINTS.to_string()..).next();
        match endpoints {
            Some((k, _)) if k.starts_with(ENDPOINTS) => ENDPOINTS,
            _ => LEGACY,
        }
    }

    /// Whether the `pathRewrite` of the endpoint `raw` (as written in the file; `name` without a
    /// trailing `/`) leaves the path after it as the target gets it, and whether it does so only
    /// for a path that starts with `/`: none, or the one rule that strips the endpoint, and no
    /// other option or file changes it.
    // ponytail: a rule with a null replacement is not in the flattened keys, so an endpoint whose
    // only rule is null reads as having none; read the parsed document if that shows up.
    fn strips(&self, prefix: &str, raw: &str, name: &str) -> (bool, bool) {
        let Read {
            keys, contested, ..
        } = self.read();
        let all = format!("{prefix}{raw}.");
        // Which file a deployment loads last is not known, so a key the files disagree on is
        // not read; `ignorePath` and `prependPath: false` change the path the target gets.
        let disputed = (contested.range(all.clone()..))
            .next()
            .is_some_and(|k| k.starts_with(&all));
        let ignored = keys
            .get(&format!("{all}ignorePath"))
            .is_some_and(|v| v != "false");
        let unprepended = keys
            .get(&format!("{all}prependPath"))
            .is_some_and(|v| v != "true");
        if disputed || ignored || unprepended {
            return (false, false);
        }
        let at = format!("{all}pathRewrite");
        let rules: Vec<_> = (self.keys().range(at.clone()..))
            .take_while(|(k, _)| k.starts_with(&at))
            .collect();
        // The pattern is a regex: the endpoint has to be plain text in it.
        let plain = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/-_".contains(c));
        match rules[..] {
            [] => (true, false),
            [(key, replacement)] if plain => {
                let pattern = key.strip_prefix(&at).and_then(|k| k.strip_prefix('.'));
                match (pattern, replacement.as_str()) {
                    (Some(p), "") if p == format!("^{PROXY}{name}") => (true, false),
                    (Some(p), "/") if p == format!("^{PROXY}{name}/") => (true, true),
                    _ => (false, false),
                }
            }
            _ => (false, false),
        }
    }

    /// `args` when their URL starts with `/api/proxy/<ep>` for an endpoint of the table: the
    /// endpoint, and the arguments with that start replaced by the endpoint's target.
    pub(super) fn proxied(&self, args: &[Value]) -> Option<Proxied> {
        let Some(Value::Str(parts)) = args.first() else {
            return None;
        };
        // The literal text the URL starts with.
        let lead = parts
            .iter()
            .take_while(|p| matches!(p, Part::Text(_)))
            .count();
        let text: String = (parts[..lead].iter())
            .filter_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        let rest = text.strip_prefix(PROXY)?;
        let (endpoint, target, slash) = self.endpoints().iter().find(|(name, ..)| {
            rest.strip_prefix(name.as_str()).is_some_and(|after| {
                (after.is_empty() && lead == parts.len()) || after.starts_with(['/', '?', '#'])
            })
        })?;
        let after = &rest[endpoint.len()..];
        let head = match target {
            Target::Env(var) if !slash || after.starts_with('/') => Part::Env(var.clone()),
            Target::Url(url) if !slash || after.starts_with('/') => Part::Text(url.clone()),
            _ => {
                return Some(Proxied {
                    endpoint: endpoint.clone(),
                    args: None,
                });
            }
        };
        let mut url = vec![head, Part::Text(after.to_string())];
        url.extend(parts[lead..].iter().cloned());
        let mut args = args.to_vec();
        args[0] = Value::Str(url);
        Some(Proxied {
            endpoint: endpoint.clone(),
            args: Some(args),
        })
    }
}

/// Reads each string endpoint of the proxy table as `{ target }`, as Backstage's proxy backend
/// does: a string value under `proxy.endpoints`, and one under a key of `proxy` that starts with
/// `/` (the legacy shape). It is applied to every file's tree, so the flattened keys of both
/// forms are `<ep>.target`.
fn expand(tree: &mut serde_json::Value) {
    use serde_json::{Map, Value};
    let Some(Value::Object(proxy)) = tree.get_mut("proxy") else {
        return;
    };
    let shorten = |table: &mut Map<String, Value>, legacy: bool| {
        for (name, value) in table.iter_mut() {
            if (!legacy || name.starts_with('/')) && value.is_string() {
                *value = Value::Object(Map::from_iter([("target".to_string(), value.take())]));
            }
        }
    };
    if let Some(Value::Object(endpoints)) = proxy.get_mut("endpoints") {
        shorten(endpoints, false);
    }
    shorten(proxy, true);
}

/// The variable a value is exactly `${VAR}` for.
fn env_var(value: &str) -> Option<String> {
    let name = value.trim().strip_prefix("${")?.strip_suffix('}')?;
    let valid = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (valid && !name.is_empty()).then(|| name.to_string())
}

/// What a proxy endpoint's `target` value is.
fn target(value: &str) -> Target {
    if let Some(var) = env_var(value) {
        return Target::Env(var);
    }
    let url = value.trim();
    let host = (url.strip_prefix("http://")).or_else(|| url.strip_prefix("https://"));
    let literal =
        host.is_some_and(|h| !h.is_empty() && !h.starts_with('/') && !h.contains(['$', '{', ' ']));
    if literal {
        Target::Url(url.trim_end_matches('/').to_string())
    } else {
        Target::Unread
    }
}

/// The includes of the `app-config*.yaml` among `configs` (`LanguagePack::config_inputs`): each
/// root grafted as the reader grafts it, so `read` is asked for every file the reader will look
/// for, and nothing else.
pub(crate) fn config_inputs<'b>(configs: &[(&str, &[u8])], read: &dyn Fn(&str) -> ConfigInput<'b>) {
    for (path, src) in configs.iter().filter(|(p, _)| is_app_config(p)) {
        includes::tree(path, src, read, &[]);
    }
}

/// The includes of Backstage's `app-config*.yaml` that no root followed, as rows of the TS and
/// JavaScript group (`Role::Unfollowed`, which `meada status` names): the including file in
/// `via`, the reason in `method` and the include as written in `fragments`. They hold no code
/// site, so each sits in the group's first file, where its rows are kept with the group's
/// (`refresh` keeps a group's rows by its files).
pub(in crate::http) fn unfollowed(code: &Code, config: &Config) -> Vec<Found> {
    let Some(&first) = code.files().keys().next() else {
        return Vec::new();
    };
    if !code.pack().extensions().contains(&"js") {
        return Vec::new();
    }
    (config.unfollowed().iter())
        .map(|u| Found {
            role: Role::Unfollowed,
            site: Site::File(first.to_string()),
            contract: None,
            via: Some(u.file.clone()),
            method: Some(u.reason.name().to_string()),
            fragments: vec![u.include.clone()],
            shape: None,
        })
        .collect()
}

/// The includes not followed that the `unfollowed` rows of an index hold, as stored (`via`,
/// `fragments`, `method`, `index::Export::unfollowed`); a row this build cannot read is left out.
pub(in crate::http) fn unfollowed_of(
    rows: &[(String, Option<String>, Option<String>)],
) -> Vec<Unfollowed> {
    (rows.iter())
        .filter_map(|(file, fragments, reason)| {
            let include: Vec<String> = serde_json::from_str(fragments.as_deref()?).ok()?;
            Some(Unfollowed {
                file: file.clone(),
                include: include.into_iter().next()?,
                reason: Reason::parse(reason.as_deref()?)?,
            })
        })
        .collect()
}

/// Whether `path` is one of Backstage's `app-config*.yaml`: the configs of every pack come in
/// together, Spring's `application.yaml` among them.
fn is_app_config(path: &str) -> bool {
    crate::lang::names_config("app-config*.yaml", path.rsplit('/').next().unwrap_or(path))
}

/// What the `app-config.yaml` among `files` (with the partials it includes) gives a call through
/// its proxy table: the size of the keys read, of the endpoints and of the includes not followed
/// (`package::hostile`).
#[cfg(test)]
pub(crate) fn probe(files: &[(&str, Vec<u8>)]) -> usize {
    let files: Vec<(&str, &[u8])> = files.iter().map(|(p, s)| (*p, s.as_slice())).collect();
    let config = Config::of(&files, &[]);
    // A miss walks every endpoint.
    let url = [Value::Str(vec![Part::Text("/api/proxy/zz/x".into())])];
    let missed = config.proxied(&url).is_some() as usize;
    let keys: usize = (config.keys().iter()).map(|(k, v)| k.len() + v.len()).sum();
    let unfollowed: usize = (config.unfollowed().iter())
        .map(|u| u.file.len() + u.include.len())
        .sum();
    keys + config.endpoints().len() + missed + unfollowed
}
