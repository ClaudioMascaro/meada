//! Backstage configuration reads in the URLs of TypeScript consumers (#617). The pack writes
//! what looks like a read, `config.getString('a.b')` or `getOptionalString`, as a `Part::Read`;
//! this module decides. The key is looked up in the repository's `app-config*.yaml` files
//! (`app-config.yaml` first, then the others in path order, the first that holds the key
//! deciding), and a value that is exactly `${VAR}` makes the read the variable `VAR`
//! (`Part::Env`), which `base_url_env` then maps. A literal value, a key no file holds and a
//! read with a member left over stay unknown, as before.
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
}

impl<'i> Config<'i> {
    pub(super) fn new(configs: &'i [(&'i str, &'i [u8])]) -> Self {
        Self {
            files: configs,
            keys: OnceCell::new(),
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
        let keys = self.keys.get_or_init(|| {
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
        });
        let name = keys
            .get(key)?
            .trim()
            .strip_prefix("${")?
            .strip_suffix('}')?;
        let valid = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        (valid && !name.is_empty()).then(|| name.to_string())
    }
}

/// Whether `path` is one of Backstage's `app-config*.yaml`: the configs of every pack come in
/// together, Spring's `application.yaml` among them.
fn is_app_config(path: &str) -> bool {
    crate::lang::names_config("app-config*.yaml", path.rsplit('/').next().unwrap_or(path))
}
