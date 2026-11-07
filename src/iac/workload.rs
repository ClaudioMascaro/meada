//! Terraform workload inputs evaluated in their root/module/instance context.

mod ecs;

use super::environment::{self, Entry};
use super::{Block, Each, Eval, Evaluated, Kind, Resolved, Val, WorkloadImage};
use hcl::Value;
use hcl_edit::expr::{Expression, ObjectKey, TraversalOperator};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// New workload code paths reject parent steps before normalization, even if still in-root.
pub(crate) fn local_path(base: &str, written: &str) -> Option<String> {
    if written.is_empty()
        || written.starts_with(['/', '~'])
        || written.contains(['\\', ':', '$', '*'])
        || written.split('/').any(|s| s == "..")
    {
        return None;
    }
    let path = super::joined(base, written)?;
    Some(if path.is_empty() { ".".into() } else { path })
}

pub(super) fn literal(value: Value) -> Option<String> {
    let text = match value {
        Value::String(v) => v,
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        _ => return None,
    };
    environment::literal(&text)
}

impl<'m> Eval<'m> {
    /// A known resource's identity is not the ARN, URL or secret contents a provider returns.
    pub(super) fn tf_environment(
        &mut self,
        instance: usize,
        each: Option<&Each>,
        expr: &'m Expression,
    ) -> (String, Option<String>, Option<String>) {
        let found = self.value(instance, each, expr, 0);
        if let Val::Known(value) = found.val {
            let target = if found.through.is_empty() {
                "Terraform environment".into()
            } else {
                format!(
                    "Terraform default {}",
                    found
                        .through
                        .iter()
                        .map(|o| o.variable.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            return (target, literal(value), None);
        }
        for entry in super::types::entries("terraform") {
            if let Some(target) = self.endpoint(instance, each, expr, &entry.ty, 0)
                && target.name.len() <= crate::config_values::VALUE_MAX
            {
                return (
                    format!("Terraform {} {}", entry.ty, target.name),
                    None,
                    Some(entry.contract(&target.name)),
                );
            }
        }
        ("Terraform environment (unresolved)".into(), None, None)
    }

    fn lambda_code(
        &mut self,
        instance: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) -> Option<Resolved> {
        let filename = &block.args.get("filename")?.expr;
        let mut found = self.value(instance, each, filename, 0);
        if !matches!(found.val, Val::Known(Value::String(_))) {
            // Follow only this exact archive output reference to an explicit local input.
            // The output itself, arbitrary data sources and ZIP members are never evaluated.
            let Expression::Traversal(t) = filename else {
                return None;
            };
            if !matches!(&t.expr, Expression::Variable(v) if v.as_str() == "data") {
                return None;
            }
            let ops = t.operators.iter().map(|o| o.value()).collect::<Vec<_>>();
            let [
                TraversalOperator::GetAttr(ty),
                TraversalOperator::GetAttr(name),
                TraversalOperator::GetAttr(output),
            ] = ops.as_slice()
            else {
                return None;
            };
            if ty.as_str() != "archive_file" || output.as_str() != "output_path" {
                return None;
            }
            let modules = self.modules;
            let mut declarations =
                modules[self.instances[instance].dir]
                    .blocks
                    .iter()
                    .filter(|b| {
                        b.kind == Kind::Data && b.ty == "archive_file" && b.name == name.as_str()
                    });
            let archive = declarations.next()?;
            if declarations.next().is_some()
                || archive.count.is_some()
                || archive.for_each.is_some()
            {
                return None;
            }
            let input = match (
                archive.args.get("source_dir"),
                archive.args.get("source_file"),
            ) {
                (Some(input), None) | (None, Some(input)) => input,
                _ => return None,
            };
            found = self.value(instance, None, &input.expr, 0);
        }
        let Val::Known(Value::String(written)) = found.val else {
            return None;
        };
        let base = if self.root == "." {
            ""
        } else {
            self.root.as_str()
        };
        Some(Resolved {
            text: local_path(base, &written)?,
            overridable: !found.through.is_empty(),
        })
    }

    pub(super) fn workload(
        &mut self,
        node: usize,
        instance: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) {
        if block.kind == Kind::Resource && block.ty == "aws_ecs_task_definition" {
            self.ecs_workload(node, instance, each, block);
            return;
        }
        if block.kind != Kind::Resource || block.ty != "aws_lambda_function" {
            return;
        }
        let mut candidate = super::Node::default();
        if let Some(arg) = block.args.get("image_uri") {
            let found = self.value(instance, each, &arg.expr, 0);
            let (text, _) = found.text();
            if text != "*" {
                candidate.images.push(WorkloadImage {
                    container: block.name.clone(),
                    image: Resolved {
                        text,
                        overridable: !found.through.is_empty(),
                    },
                    through: found.through.into_iter().collect(),
                });
            }
        }
        candidate.build = self.lambda_code(instance, each, block);
        if let Some(arg) = block.args.get("handler") {
            let found = self.value(instance, each, &arg.expr, 0);
            candidate.handler = Some(Resolved {
                text: found.text().0,
                overridable: !found.through.is_empty(),
            });
        }
        candidate.environment = self.lambda_variables(
            instance,
            each,
            &block.name,
            candidate.images.first().map(|i| i.image.text.as_str()),
            self.root_left
                .min(self.left)
                .saturating_sub(candidate.workload_size())
                .min(environment::ENVIRONMENT_BYTES_MAX),
            block
                .nested
                .iter()
                .filter(|b| b.ident.as_str() == "environment")
                .filter_map(|b| b.body.get_attribute("variables").map(|a| &a.value)),
        );
        if self.charge(candidate.workload_size()) {
            self.nodes[node].images = candidate.images;
            self.nodes[node].environment = candidate.environment;
            self.nodes[node].build = candidate.build;
            self.nodes[node].handler = candidate.handler;
        }
    }
    fn lambda_variables(
        &mut self,
        instance: usize,
        each: Option<&Each>,
        container: &str,
        image: Option<&str>,
        room: usize,
        expressions: impl Iterator<Item = &'m Expression>,
    ) -> Vec<Entry> {
        let mut entries = BTreeMap::new();
        let mut room = room;
        let mut put = |name: &str, found: (String, Option<String>, Option<String>)| {
            // An unread or duplicate key can replace any literal entry. Preserve its
            // unknown scope for HTTP too, without retaining a partial environment.
            let complete = name != "*" && !entries.contains_key(name);
            let (name, (target, value, contract)) = if complete {
                (name, found)
            } else {
                entries.clear();
                (
                    "*",
                    ("Terraform environment (unresolved)".into(), None, None),
                )
            };
            let bytes = container.len()
                + image.map_or(0, str::len)
                + 2 * name.len()
                + target.len()
                + value.as_ref().map_or(0, String::len)
                + contract.as_ref().map_or(0, String::len);
            let Some(left) = room.checked_sub(bytes) else {
                entries.clear();
                return false;
            };
            room = left;
            entries.insert(
                name.to_string(),
                Entry {
                    container: container.into(),
                    image: image.map(str::to_string),
                    name: name.into(),
                    target,
                    value,
                    contract,
                },
            );
            complete
        };
        'environment: for expression in expressions {
            if let Expression::Object(values) = expression {
                for (key, value) in values {
                    let name = match key {
                        ObjectKey::Ident(id) => Cow::Borrowed(id.as_str()),
                        ObjectKey::Expression(Expression::String(s)) => {
                            Cow::Borrowed(s.value().as_str())
                        }
                        ObjectKey::Expression(expr) => {
                            let Val::Known(Value::String(name)) =
                                self.value(instance, each, expr, 0).val
                            else {
                                put("*", (String::new(), None, None));
                                break 'environment;
                            };
                            Cow::Owned(name)
                        }
                    };
                    if !put(&name, self.tf_environment(instance, each, value.expr())) {
                        break 'environment;
                    }
                }
            } else {
                let Evaluated {
                    val: Val::Known(Value::Object(values)),
                    ..
                } = self.value(instance, each, expression, 0)
                else {
                    put("*", (String::new(), None, None));
                    break 'environment;
                };
                for (name, value) in values {
                    if !put(
                        &name,
                        ("Terraform environment".into(), literal(value), None),
                    ) {
                        break 'environment;
                    }
                }
            }
        }
        entries.into_values().collect()
    }

    pub(in crate::iac) fn module_lambda(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
        gates: &BTreeSet<super::Overridable>,
    ) {
        let mut mode_through = gates.clone();
        let mode = match block.args.get("package_type") {
            None => Some("Zip".to_string()),
            Some(arg) => {
                let found = self.value(at, each, &arg.expr, 0);
                mode_through.extend(found.through);
                match found.val {
                    Val::Known(Value::String(mode)) => Some(mode),
                    _ => None,
                }
            }
        };
        let mut candidate = super::Node::default();
        if mode.as_deref() == Some("Image")
            && let Some(arg) = block.args.get("image_uri")
        {
            let mut found = self.value(at, each, &arg.expr, 0);
            let (text, _) = found.text();
            found.through.extend(mode_through.iter().cloned());
            if text != "*" {
                candidate.images.push(WorkloadImage {
                    container: block.name.clone(),
                    image: Resolved {
                        text,
                        overridable: !found.through.is_empty(),
                    },
                    through: found.through.into_iter().collect(),
                });
            }
        }
        let mut code = None;
        if mode.as_deref() == Some("Zip") {
            let existing = block
                .args
                .get("local_existing_package")
                .map(|arg| self.value(at, each, &arg.expr, 0));
            let (key, file, found) = match existing {
                Some(
                    found @ Evaluated {
                        val: Val::Known(Value::String(_)),
                        ..
                    },
                ) => ("local_existing_package", true, Some(found)),
                None
                | Some(Evaluated {
                    val: Val::Known(Value::Null),
                    ..
                }) => {
                    let mut through = mode_through.clone();
                    let package =
                        self.module_bool(at, each, block, "create_package", true, &mut through);
                    let remote = block
                        .args
                        .get("s3_existing_package")
                        .map(|arg| self.value(at, each, &arg.expr, 0));
                    let local = remote.is_none_or(|v| matches!(v.val, Val::Known(Value::Null)));
                    let found = if package == Some(true) && local {
                        block.args.get("source_path").map(|arg| {
                            let mut found = self.value(at, each, &arg.expr, 0);
                            found.through.extend(through);
                            found
                        })
                    } else {
                        None
                    };
                    ("source_path", false, found)
                }
                _ => ("local_existing_package", true, None),
            };
            if let Some(Evaluated {
                val: Val::Known(Value::String(path)),
                through,
            }) = found
            {
                let base = if self.root == "." {
                    ""
                } else {
                    self.root.as_str()
                };
                if let Some(text) = local_path(base, &path) {
                    candidate.build = Some(Resolved {
                        text,
                        overridable: !through.is_empty() || !mode_through.is_empty(),
                    });
                    code = Some(super::mapping::Code {
                        key: key.into(),
                        file,
                    });
                }
            }
        }
        if let Some(arg) = block.args.get("handler") {
            let found = self.value(at, each, &arg.expr, 0);
            candidate.handler = Some(Resolved {
                text: found.text().0,
                overridable: !found.through.is_empty(),
            });
        }
        candidate.environment = self.lambda_variables(
            at,
            each,
            &block.name,
            candidate.images.first().map(|i| i.image.text.as_str()),
            self.root_left
                .min(self.left)
                .saturating_sub(candidate.workload_size())
                .min(environment::ENVIRONMENT_BYTES_MAX),
            block
                .args
                .get("environment_variables")
                .into_iter()
                .map(|arg| &arg.expr),
        );
        let needed = candidate.workload_size() + code.as_ref().map_or(0, |c| c.key.len() + 24);
        if self.charge(needed) {
            self.nodes[node].images = candidate.images;
            self.nodes[node].environment = candidate.environment;
            self.nodes[node].build = candidate.build;
            self.nodes[node].handler = candidate.handler;
            if let Some(projection) = &mut self.nodes[node].provisioned {
                projection.code = code;
            }
        }
    }
}
