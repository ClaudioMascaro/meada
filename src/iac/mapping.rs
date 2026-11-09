//! Repository declarations mapping private abstractions onto the embedded type table.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::types;

mod modules;

// ponytail: a repository's abstraction inventory is small; 256 rows bounds every lookup.
const MAPPINGS_MAX: usize = 256;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(try_from = "Raw")]
pub(crate) struct Config {
    pub modules: Vec<Module>,
    pub custom: Vec<Custom>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(default, rename = "module")]
    modules: Vec<Module>,
    #[serde(default, rename = "custom_resource")]
    custom: Vec<Custom>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Module {
    pub source: Option<String>,
    pub source_prefix: Option<String>,
    pub entry: String,
    pub input: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Custom {
    pub group: String,
    pub kind: String,
    pub entry: String,
    pub key_path: String,
}

/// A declared module code input, checked on disk only by the image kind during indexing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Code {
    pub key: String,
    pub file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Binding {
    pub contract: String,
    pub exchange: String,
    pub pattern: String,
}

impl Binding {
    pub(crate) fn size(&self) -> usize {
        self.contract.len() + self.exchange.len() + self.pattern.len()
    }
}

fn text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::config_values::VALUE_MAX
        && !value.chars().any(char::is_control)
}

fn path(value: &str) -> bool {
    text(value)
        && crate::config_values::sites::parts_without_selectors(value)
            .is_some_and(|steps| !steps.is_empty())
}

fn known(entry: &str, workload: bool) -> Result<(), String> {
    if !text(entry) {
        return Err("an IaC type table entry must be a bounded literal string".into());
    }
    if types::named(entry).is_some() {
        return Ok(());
    }
    if workload && let Some(row) = types::named_workload(entry) {
        return if row.format == "terraform"
            && matches!(
                row.ty.as_str(),
                "aws_lambda_function" | "aws_ecs_task_definition"
            ) {
            Ok(())
        } else {
            Err(format!(
                "IaC workload entry `{entry}` is not supported for a module mapping"
            ))
        };
    }
    Err(format!("unknown IaC type table entry `{entry}`"))
}

impl TryFrom<Raw> for Config {
    type Error = String;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        if raw.modules.len().saturating_add(raw.custom.len()) > MAPPINGS_MAX {
            return Err(format!("IaC mappings exceed {MAPPINGS_MAX} entries"));
        }
        let mut sources = BTreeSet::new();
        for module in &raw.modules {
            known(&module.entry, true)?;
            let source = match (&module.source, &module.source_prefix) {
                (Some(source), None) => (false, source),
                (None, Some(source)) => (true, source),
                _ => {
                    return Err("an IaC module mapping needs one of source or source_prefix".into());
                }
            };
            if !text(source.1) || !path(&module.input) {
                return Err(
                    "an IaC module mapping needs a bounded source and an input key path".into(),
                );
            }
            if !sources.insert(source) {
                return Err("duplicate IaC module mapping source".into());
            }
        }
        let mut kinds = BTreeSet::new();
        for custom in &raw.custom {
            known(&custom.entry, false)?;
            if !text(&custom.group) || !text(&custom.kind) || !path(&custom.key_path) {
                return Err(
                    "an IaC custom resource mapping needs group, kind and a key path".into(),
                );
            }
            if !kinds.insert((&custom.group, &custom.kind)) {
                return Err("duplicate IaC custom resource group and kind".into());
            }
        }
        Ok(Self {
            modules: raw.modules,
            custom: raw.custom,
        })
    }
}

fn prefix(source: &str, prefix: &str) -> bool {
    source.strip_prefix(prefix).is_some_and(|rest| {
        rest.is_empty() || prefix.ends_with('/') || rest.starts_with(['/', '?'])
    })
}

impl Config {
    pub(crate) fn module(&self, source: &str) -> Option<&Module> {
        self.modules
            .iter()
            .find(|m| m.source.as_deref() == Some(source))
            .or_else(|| {
                self.modules
                    .iter()
                    .filter(|m| {
                        m.source_prefix
                            .as_deref()
                            .is_some_and(|p| prefix(source, p))
                    })
                    .max_by_key(|m| m.source_prefix.as_ref().map_or(0, String::len))
            })
    }

    pub(crate) fn custom(&self, group: &str, kind: &str) -> Option<&Custom> {
        self.custom
            .iter()
            .find(|c| c.group == group && c.kind == kind)
    }

    pub(crate) fn stamp(&self) -> crate::Result<String> {
        let mut stamp = blake3::Hasher::new();
        stamp.update(types::stamp().as_bytes());
        stamp.update(&serde_json::to_vec(self)?);
        Ok(stamp.finalize().to_hex().to_string())
    }
}
