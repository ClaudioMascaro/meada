//! ECS container definitions use the shared Terraform evaluator and its JSON encoder.

use super::super::environment::{self, Entry};
use super::super::{Block, Each, Eval, Evaluated, Node, Resolved, Val, WorkloadImage};
use hcl::Value;
use hcl_edit::expr::{Expression, ObjectKey};
use serde_json::Value as Json;
use std::collections::BTreeMap;

fn field<'a>(expr: &'a Expression, key: &str) -> Option<&'a Expression> {
    let Expression::Object(object) = expr else {
        return None;
    };
    let mut matches = object.iter().filter_map(|(k, v)| {
        let name = match k {
            ObjectKey::Ident(i) => i.as_str(),
            ObjectKey::Expression(Expression::String(s)) => s.value(),
            _ => return None,
        };
        (name == key).then(|| v.expr())
    });
    let found = matches.next()?;
    matches.next().is_none().then_some(found)
}

/// Charges repeated strings before copying them. An incomplete environment never picks a
/// partial name; images remain useful independently of unknown or oversized environments.
struct Facts {
    node: Node,
    entries: BTreeMap<(String, String), Entry>,
    left: usize,
    env_left: usize,
    env_bytes: usize,
    env_unread: bool,
}

impl Facts {
    fn new(left: usize) -> Self {
        Self {
            node: Node::default(),
            entries: BTreeMap::new(),
            left,
            env_left: environment::ENVIRONMENT_BYTES_MAX,
            env_bytes: 0,
            env_unread: false,
        }
    }
    fn unread(&mut self) {
        self.entries.clear();
        self.left += self.env_bytes;
        self.env_bytes = 0;
        self.env_unread = true;
    }
    fn image(
        &mut self,
        container: &str,
        text: &str,
        overridable: bool,
        through: &[super::super::Overridable],
    ) -> bool {
        let cost = container.len()
            + text.len()
            + through
                .iter()
                .map(|o| o.path.len() + o.variable.len() + 16)
                .sum::<usize>();
        if cost > self.left {
            self.unread();
        }
        let Some(left) = self.left.checked_sub(cost) else {
            return false;
        };
        self.left = left;
        self.node.images.push(WorkloadImage {
            container: container.into(),
            image: Resolved {
                text: text.into(),
                overridable,
            },
            through: through.to_vec(),
        });
        true
    }
    fn entry(
        &mut self,
        container: &str,
        image: Option<&str>,
        name: &str,
        (target, value, contract): (String, Option<String>, Option<String>),
    ) {
        if self.env_unread {
            return;
        }
        // The temporary map owns a second copy of the container and variable keys.
        let cost = 2 * (container.len() + name.len())
            + image.map_or(0, str::len)
            + target.len()
            + value.as_ref().map_or(0, String::len)
            + contract.as_ref().map_or(0, String::len);
        let (Some(left), Some(env_left)) =
            (self.left.checked_sub(cost), self.env_left.checked_sub(cost))
        else {
            self.unread();
            return;
        };
        self.left = left;
        self.env_left = env_left;
        self.env_bytes += cost;
        self.entries.insert(
            (container.into(), name.into()),
            Entry {
                container: container.into(),
                image: image.map(str::to_string),
                name: name.into(),
                target,
                value,
                contract,
            },
        );
    }
    fn finish(mut self) -> Node {
        self.node.environment = self.entries.into_values().collect();
        self.node
    }
}

fn scalar(value: &Json) -> Option<String> {
    let text = match value {
        Json::String(s) => return environment::literal(s),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => n.to_string(),
        _ => return None,
    };
    environment::literal(&text)
}

impl<'m> Eval<'m> {
    fn ecs_fields(&mut self, expr: &Expression, scans: usize) -> bool {
        let cost = match expr {
            Expression::Object(o) => o.len().saturating_mul(scans),
            _ => 1,
        };
        if let Some(left) = self.work.checked_sub(cost) {
            self.work = left;
            self.work != 0
        } else {
            self.work = 0;
            false
        }
    }

    fn ecs_environment(
        &mut self,
        facts: &mut Facts,
        at: usize,
        each: Option<&Each>,
        location: (&str, Option<&str>),
        expr: &'m Expression,
        secret: bool,
    ) {
        let (container, image) = location;
        if facts.env_unread {
            return;
        }
        if let Expression::Array(entries) = expr {
            for entry in entries {
                if !self.ecs_fields(entry, 2) {
                    facts.unread();
                    return;
                }
                let Some(name) = field(entry, "name") else {
                    facts.unread();
                    return;
                };
                let Val::Known(Value::String(name)) = self.value(at, each, name, 0).val else {
                    facts.unread();
                    return;
                };
                let value = if secret {
                    ("ECS secret reference (redacted)".into(), None, None)
                } else if let Some(value) = field(entry, "value") {
                    self.tf_environment(at, each, value)
                } else {
                    ("ECS environment (unresolved)".into(), None, None)
                };
                facts.entry(container, image, &name, value);
            }
        } else {
            let Val::Known(value) = self.value(at, each, expr, 0).val else {
                facts.unread();
                return;
            };
            let Ok(json) = serde_json::to_value(value) else {
                facts.unread();
                return;
            };
            Self::ecs_json_environment(facts, container, image, &json, secret);
        }
    }
    fn ecs_json_environment(
        facts: &mut Facts,
        container: &str,
        image: Option<&str>,
        value: &Json,
        secret: bool,
    ) {
        let Some(entries) = value.as_array() else {
            facts.unread();
            return;
        };
        for entry in entries {
            let Some(name) = entry.get("name").and_then(Json::as_str) else {
                facts.unread();
                return;
            };
            let value = if secret {
                ("ECS secret reference (redacted)".into(), None, None)
            } else {
                (
                    "Terraform ECS environment".into(),
                    entry.get("value").and_then(scalar),
                    None,
                )
            };
            facts.entry(container, image, name, value);
        }
    }
    pub(super) fn ecs_workload(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) {
        let Some(arg) = block.args.get("container_definitions") else {
            return;
        };
        let mut facts = Facts::new(self.root_left.min(self.left));
        let direct = match &arg.expr {
            Expression::FuncCall(f)
                if f.name.name.as_str() == "jsonencode"
                    && !f.name.is_namespaced()
                    && !f.args.expand_final()
                    && f.args.len() == 1 =>
            {
                match f.args.iter().next() {
                    Some(Expression::Array(a)) => Some(a),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(containers) = direct {
            for (index, entry) in containers.iter().enumerate() {
                if !self.ecs_fields(entry, 4) {
                    facts.unread();
                    break;
                }
                let name = field(entry, "name")
                    .and_then(|e| match self.value(at, each, e, 0).val {
                        Val::Known(Value::String(s)) => Some(s),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("container[{index}]"));
                let image = field(entry, "image").and_then(|expr| {
                    let found = self.value(at, each, expr, 0);
                    let (text, _) = found.text();
                    (text != "*").then(|| {
                        (
                            Resolved {
                                text,
                                overridable: !found.through.is_empty(),
                            },
                            found.through.into_iter().collect::<Vec<_>>(),
                        )
                    })
                });
                if let Some((image, through)) = &image
                    && !facts.image(&name, &image.text, image.overridable, through)
                {
                    break;
                }
                let image = image.as_ref().map(|(i, _)| i.text.as_str());
                for (key, secret) in [("environment", false), ("secrets", true)] {
                    if let Some(expr) = field(entry, key) {
                        self.ecs_environment(&mut facts, at, each, (&name, image), expr, secret);
                    }
                }
            }
        } else {
            let Evaluated {
                val: Val::Known(Value::String(text)),
                through,
            } = self.value(at, each, &arg.expr, 0)
            else {
                return;
            };
            let Ok(Json::Array(containers)) = serde_json::from_str::<Json>(&text) else {
                return;
            };
            let through = through.into_iter().collect::<Vec<_>>();
            for (index, entry) in containers.iter().enumerate() {
                let fallback = format!("container[{index}]");
                let name = entry
                    .get("name")
                    .and_then(Json::as_str)
                    .unwrap_or(&fallback);
                let image = entry.get("image").and_then(Json::as_str);
                if let Some(text) = image
                    && !facts.image(name, text, !through.is_empty(), &through)
                {
                    break;
                }
                for (key, secret) in [("environment", false), ("secrets", true)] {
                    if let Some(value) = entry.get(key) {
                        Self::ecs_json_environment(&mut facts, name, image, value, secret);
                    }
                }
            }
        }
        let facts = facts.finish();
        if self.charge(facts.workload_size()) {
            self.nodes[node].images = facts.images;
            self.nodes[node].environment = facts.environment;
        }
    }
}
