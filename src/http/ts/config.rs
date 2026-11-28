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

pub(crate) struct Config<'i> {
    files: &'i [(&'i str, &'i [u8])],
    symlinks: &'i [&'i str],
    keys: OnceCell<Read>,
    endpoints: OnceCell<Vec<Endpoint>>,
}

struct Read {
    keys: Properties,
    contested: BTreeSet<String>,
    literals: BTreeMap<String, BTreeSet<String>>,
    unfollowed: Vec<Unfollowed>,
    dropped: bool,
    endpoints_table: bool,
}

type Endpoint = (String, Target, bool);

enum Target {
    Env(String),
    Url(String),
    Unread,
}

pub(super) struct Proxied {
    pub(super) endpoint: String,
    pub(super) args: Option<Vec<Value>>,
}

const PROXY: &str = "/api/proxy";
// ponytail: an `$env` beside another key there is unread too (Backstage refuses the config), so
// it hides the legacy keys, but it is no include and `meada status` names nothing; name it as an
// unfollowed row if that shows up.
const TABLE_PATHS: &[&[&str]] = &[&[], &["proxy"], &["proxy", "endpoints"]];

const ENDPOINTS: &str = "proxy.endpoints.";
const LEGACY: &str = "proxy.";

impl<'i> Config<'i> {
    pub(crate) fn new(indexed: &Indexed<'i>) -> Self {
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

    pub(super) fn decided<R>(
        &self,
        args: Vec<Value>,
        mut read: impl FnMut(&mut [Value], bool) -> Option<R>,
    ) -> Option<Vec<R>> {
        if let Some(Value::Str(parts)) = args.first()
            && let Some(index) = parts.iter().position(|part| {
                !matches!(part, Part::StripStart)
                    && !matches!(part, Part::Text(text) if text.is_empty())
            })
            && let Part::RecordValues { key: None, values } = &parts[index]
        {
            let mut readings = Vec::new();
            for value in values {
                let mut args = args.clone();
                if let Value::Str(parts) = &mut args[0] {
                    parts.splice(index..=index, value.iter().cloned());
                }
                readings.extend(self.configured(args, |args, _| read(args, true))?);
            }
            return Some(readings);
        }
        self.configured(args, read)
    }

    fn configured<R>(
        &self,
        mut args: Vec<Value>,
        mut read: impl FnMut(&mut [Value], bool) -> Option<R>,
    ) -> Option<Vec<R>> {
        let Some(Value::Str(parts)) = args.first_mut() else {
            return Some(vec![read(&mut args, false)?]);
        };
        let base = parts.iter().position(|p| {
            !matches!(p, Part::StripStart) && !matches!(p, Part::Text(text) if text.is_empty())
        });
        let mut choices = None;
        for (i, part) in parts.iter_mut().enumerate() {
            if let Part::Read {
                callee, key, then, ..
            } = part
            {
                if then.is_empty() && !callee.ends_with(".getBaseUrl") {
                    if Some(i) == base && !self.literals(key).is_empty() {
                        choices = Some((i, self.literals(key)));
                    }
                    *part = self
                        .keys()
                        .get(key)
                        .and_then(|value| env_var(value))
                        .map_or(Part::Hole, Part::Env);
                } else {
                    *part = Part::Hole;
                }
            }
        }
        let Some((i, values)) = choices else {
            return Some(vec![read(&mut args, false)?]);
        };
        let uncertain = values.len() > 1 || !self.whole();
        values
            .iter()
            .map(|value| {
                let mut args = args.clone();
                if let Value::Str(parts) = &mut args[0] {
                    parts[i] = if let Some(name) = env_var(value) {
                        Part::Env(name)
                    } else if value.contains("${") {
                        Part::Hole
                    } else {
                        Part::Text(value.clone())
                    };
                }
                read(&mut args, uncertain)
            })
            .collect()
    }

    pub(crate) fn literals(&self, key: &str) -> &BTreeSet<String> {
        static NONE: BTreeSet<String> = BTreeSet::new();
        self.read().literals.get(key).unwrap_or(&NONE)
    }

    pub(crate) fn whole(&self) -> bool {
        !self.read().dropped && self.read().unfollowed.is_empty()
    }

    fn keys(&self) -> &Properties {
        &self.read().keys
    }

    fn read(&self) -> &Read {
        self.keys.get_or_init(|| {
            let by_path: BTreeMap<&str, &[u8]> = self.files.iter().copied().collect();
            let read = |path: &str| match by_path.get(path) {
                Some(src) => ConfigInput::Read((*src).into()),
                None if self.symlinks.contains(&path) => ConfigInput::Symlink,
                None => ConfigInput::Missing,
            };
            let mut keys = BTreeMap::new();
            let mut contested = BTreeSet::new();
            let mut literals: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            let mut unfollowed = BTreeSet::new();
            let mut endpoints_table = false;
            let mut dropped = false;
            let mut reading = includes::Reading::default();
            for (path, src) in roots(self.files) {
                let grafted = includes::tree(path, src, &read, TABLE_PATHS, &mut reading);
                unfollowed.extend(grafted.unfollowed);
                endpoints_table |= grafted.watched_unread;
                let Some(mut tree) = grafted.tree else {
                    dropped = true;
                    continue;
                };
                expand(&mut tree);
                endpoints_table |= !tree["proxy"]["endpoints"].is_null();
                for (k, v) in flatten(&tree) {
                    literals.entry(k.clone()).or_default().insert(v.clone());
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
                literals,
                unfollowed: unfollowed.into_iter().collect(),
                dropped,
                endpoints_table,
            }
        })
    }

    fn unfollowed(&self) -> &[Unfollowed] {
        &self.read().unfollowed
    }

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

    // ponytail: a rule with a null replacement is not in the flattened keys, so an endpoint whose
    // only rule is null reads as having none; read the parsed document if that shows up.
    fn strips(&self, prefix: &str, raw: &str, name: &str) -> (bool, bool) {
        let Read {
            keys, contested, ..
        } = self.read();
        let all = format!("{prefix}{raw}.");
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

    pub(super) fn proxied(&self, args: &[Value]) -> Option<Proxied> {
        let Some(Value::Str(parts)) = args.first() else {
            return None;
        };
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

fn env_var(value: &str) -> Option<String> {
    let name = value.trim().strip_prefix("${")?.strip_suffix('}')?;
    let valid = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (valid && !name.is_empty()).then(|| name.to_string())
}

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

pub(crate) fn config_inputs<'b>(configs: &[(&str, &[u8])], read: &dyn Fn(&str) -> ConfigInput<'b>) {
    let mut reading = includes::Reading::default();
    for (path, src) in roots(configs) {
        includes::tree(path, src, read, &[], &mut reading);
    }
}

fn roots<'a, 'f>(files: &'a [(&'f str, &'f [u8])]) -> Vec<&'a (&'f str, &'f [u8])> {
    let mut roots: Vec<_> = files.iter().filter(|(p, _)| is_app_config(p)).collect();
    roots.sort_by_key(|(p, _)| (p.rsplit('/').next() != Some("app-config.yaml"), *p));
    roots
}

pub(in crate::http) fn unfollowed(code: &Code, config: &Config) -> Vec<Found> {
    let Some(&first) = code.files().keys().next() else {
        return Vec::new();
    };
    if !code.pack().extensions().contains(&"js") {
        return Vec::new();
    }
    (config.unfollowed().iter())
        .map(|u| Found {
            identity: None,
            role: Role::Unfollowed,
            site: Site::File(first.to_string()),
            contract: None,
            via: Some(u.file.clone()),
            method: Some(u.reason.name().to_string()),
            fragments: vec![u.include.clone()],
            shape: None,
            lead: None,
            cap: None,
            client: None,
        })
        .collect()
}

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

fn is_app_config(path: &str) -> bool {
    crate::lang::names_config("app-config*.yaml", path.rsplit('/').next().unwrap_or(path))
}

#[cfg(test)]
pub(crate) fn probe(files: &[(&str, Vec<u8>)]) -> usize {
    let files: Vec<(&str, &[u8])> = files.iter().map(|(p, s)| (*p, s.as_slice())).collect();
    let config = Config::of(&files, &[]);
    let url = [Value::Str(vec![Part::Text("/api/proxy/zz/x".into())])];
    let missed = config.proxied(&url).is_some() as usize;
    let keys: usize = (config.keys().iter()).map(|(k, v)| k.len() + v.len()).sum();
    let literals: usize = (config.keys().keys())
        .map(|k| config.literals(k).iter().map(String::len).sum::<usize>())
        .sum();
    let unfollowed: usize = (config.unfollowed().iter())
        .map(|u| u.file.len() + u.include.len())
        .sum();
    keys + literals + config.endpoints().len() + missed + unfollowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::MAX_BYTES;

    #[test]
    fn config_inputs_ask_for_what_the_reader_follows_past_the_reading_limit() {
        let large = format!("url: u\npad: '{}'\n", "x".repeat(MAX_BYTES - 32));
        let small = format!("url: q\npad: '{}'\n", "x".repeat(100));
        let partials =
            BTreeMap::from([("p.yaml", large.as_bytes()), ("z/q.yaml", small.as_bytes())]);
        let mut roots: Vec<(String, &[u8])> = (0..4)
            .map(|i| {
                (
                    format!("app-config.x{i}.yaml"),
                    &b"a: { $include: p.yaml }\n"[..],
                )
            })
            .collect();
        roots.push(("z/app-config.yaml".into(), b"b: { $include: q.yaml }\n"));
        let mut files: Vec<(&str, &[u8])> = (roots.iter()).map(|(p, s)| (p.as_str(), *s)).collect();
        let asked = std::cell::RefCell::new(BTreeSet::new());
        let read = |path: &str| {
            asked.borrow_mut().insert(path.to_string());
            (partials.get(path))
                .map_or(ConfigInput::Missing, |src| ConfigInput::Read((*src).into()))
        };
        config_inputs(&files, &read);
        let asked = asked.into_inner();
        files.extend(
            (partials.iter())
                .filter(|(p, _)| asked.contains(**p))
                .map(|(p, s)| (*p, *s)),
        );
        let config = Config::of(&files, &[]);
        let refused: Vec<_> = (config.unfollowed().iter())
            .map(|u| (u.file.as_str(), u.include.as_str(), u.reason))
            .collect();
        assert_eq!(refused, [("app-config.x3.yaml", "p.yaml", Reason::Size)]);
    }

    #[test]
    fn only_roots_named_app_config_yaml_are_read_first() {
        let large = format!("url: u\npad: '{}'\n", "x".repeat(MAX_BYTES - 32));
        let include = &b"a: { $include: p.yaml }\n"[..];
        let mut files: Vec<(&str, &[u8])> = [
            "a/app-config.yaml",
            "b/app-config.yaml",
            "c/app-config.yaml",
            "app-config.local.yaml",
            "app-config.zz-app-config.yaml",
        ]
        .map(|p| (p, include))
        .to_vec();
        for partial in ["a/p.yaml", "b/p.yaml", "c/p.yaml", "p.yaml"] {
            files.push((partial, large.as_bytes()));
        }
        let config = Config::of(&files, &[]);
        let refused: Vec<_> = (config.unfollowed().iter())
            .map(|u| (u.file.as_str(), u.reason))
            .collect();
        assert_eq!(refused, [("app-config.zz-app-config.yaml", Reason::Size)]);
    }

    #[test]
    fn an_include_refused_for_its_size_reads_no_file_already_read() {
        let large = format!("url: u\npad: '{}'\n", "x".repeat(MAX_BYTES - 32));
        let root =
            &b"a: { $include: p.yaml }\nb: { $include: p.yaml }\nc: { $include: p.yaml }\n"[..];
        let roots: Vec<String> = (0..6).map(|i| format!("app-config.x{i}.yaml")).collect();
        let files: Vec<(&str, &[u8])> = roots.iter().map(|p| (p.as_str(), root)).collect();
        let reads = std::cell::Cell::new(0);
        let read = |path: &str| {
            assert_eq!(path, "p.yaml");
            reads.set(reads.get() + 1);
            ConfigInput::Read(large.as_bytes().into())
        };
        config_inputs(&files, &read);
        assert_eq!(reads.get(), 4);
    }
}
