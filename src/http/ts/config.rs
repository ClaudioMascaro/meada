//! Backstage configuration reads in the URLs of TypeScript consumers (#617). The pack writes
//! what looks like a read, `config.getString('a.b')` or `getOptionalString`, as a `Part::Read`;
//! this module decides. The key is looked up in the repository's `app-config*.yaml` files
//! (`app-config.yaml` first, then the others in path order, the first that holds the key
//! deciding), and a value that is exactly `${VAR}` makes the read the variable `VAR`
//! (`Part::Env`), which `base_url_env` then maps. A literal value, a key no file holds and a
//! read with a member left over stay unknown, as before.
//!
//! The same files hold Backstage's proxy table (#616): `proxy.endpoints['/<ep>'].target`. A URL
//! that starts with `/api/proxy/<ep>` (the proxy plugin's mount, also what `getBaseUrl('proxy')`
//! folds to) is a call to that target: a `${VAR}` target is the variable `VAR`, an `http(s)://`
//! target is that host (with its path), and the rest of the URL follows it. Any other target
//! (a template around the variable, a default, no `http`), and an endpoint with a `pathRewrite`
//! (the path after it would not be the target's), leave the URL as written and the call
//! unlinked, and `meada status` says so.
// ponytail: every `app-config*.yaml` of the repository serves every file of it, whatever the
// directory, and profiles are not merged: the first file holding the key wins. Scope the lookup
// by directory if a monorepo's apps keep different values under one key.

use std::cell::OnceCell;
use std::collections::BTreeMap;

use crate::jvm::spring::{Properties, read_yaml};
use crate::lang::{Part, Value};

/// The reads of one extraction; the configuration files are parsed the first time one asks.
pub(super) struct Config<'i> {
    files: &'i [(&'i str, &'i [u8])],
    keys: OnceCell<Properties>,
    endpoints: OnceCell<Vec<(String, Target)>>,
}

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

impl<'i> Config<'i> {
    pub(super) fn new(configs: &'i [(&'i str, &'i [u8])]) -> Self {
        Self {
            files: configs,
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
        self.keys.get_or_init(|| {
            let mut files: Vec<_> = self
                .files
                .iter()
                .filter(|(p, _)| is_app_config(p))
                .collect();
            // `app-config.yaml` is the base the others override.
            files.sort_by_key(|(p, _)| (!p.ends_with("app-config.yaml"), *p));
            let mut keys = BTreeMap::new();
            for (_, src) in files {
                let Ok(src) = std::str::from_utf8(src) else {
                    continue;
                };
                for (k, v) in read_yaml(src) {
                    keys.entry(k).or_insert(v);
                }
            }
            keys
        })
    }

    /// `proxy.endpoints['/<ep>'].target`, longest endpoint first.
    // ponytail: only the `target` form is read: the string shorthand (`'/ep': 'https://...'`) is
    // indistinguishable from a dotted endpoint name in the flattened keys; read it from the
    // parsed document if configs in the wild use it.
    fn endpoints(&self) -> &[(String, Target)] {
        self.endpoints.get_or_init(|| {
            let mut out: Vec<_> = (self.keys().iter())
                .filter_map(|(key, value)| {
                    let name = key
                        .strip_prefix("proxy.endpoints.")?
                        .strip_suffix(".target")?;
                    let name = name.trim_end_matches('/');
                    // `|` is the separator of a consumer's twin readings (`keep_applicable`).
                    if !name.starts_with('/') || name.contains('|') {
                        return None;
                    }
                    // The proxy rewrites the path after the endpoint, which Meada does not follow.
                    let rewrites = format!("proxy.endpoints.{name}.pathRewrite");
                    let rewritten = self.keys().keys().any(|k| k.starts_with(&rewrites));
                    let target = if rewritten {
                        Target::Unread
                    } else {
                        target(value)
                    };
                    Some((name.to_string(), target))
                })
                .collect();
            out.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
            out
        })
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
        let (endpoint, target) = self.endpoints().iter().find(|(name, _)| {
            rest.strip_prefix(name.as_str()).is_some_and(|after| {
                (after.is_empty() && lead == parts.len()) || after.starts_with(['/', '?', '#'])
            })
        })?;
        let after = &rest[endpoint.len()..];
        let head = match target {
            Target::Env(var) => Part::Env(var.clone()),
            Target::Url(url) => Part::Text(url.clone()),
            Target::Unread => {
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
    let literal = host.is_some_and(|h| !h.starts_with('/') && !h.contains(['$', '{', ' ']));
    if literal {
        Target::Url(url.trim_end_matches('/').to_string())
    } else {
        Target::Unread
    }
}

/// Whether `path` is one of Backstage's `app-config*.yaml`: the configs of every pack come in
/// together, Spring's `application.yaml` among them.
fn is_app_config(path: &str) -> bool {
    crate::lang::names_config("app-config*.yaml", path.rsplit('/').next().unwrap_or(path))
}
