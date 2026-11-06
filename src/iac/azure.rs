//! Azure Terraform projections, using the contract kind's identity policy (#669).
use super::*;

impl<'m> Eval<'m> {
    pub(super) fn azure_projection(&mut self, node: usize, block: &'m Block) {
        if block.kind != Kind::Resource
            || !block.ty.starts_with("azurerm_")
            || block.ty == "azurerm_servicebus_subscription"
        {
            return;
        }
        let Some(entry) = types::entry("terraform", &block.ty) else {
            return;
        };
        let Some(name) = self.nodes[node].name.as_deref() else {
            return;
        };
        let bytes = name.len()
            + self.nodes[node]
                .overridable
                .iter()
                .map(|o| o.path.len() + o.variable.len() + 16)
                .sum::<usize>();
        if !self.charge(bytes) {
            return;
        }
        let name = self.nodes[node].name.as_deref().unwrap();
        let projection = Projection {
            name: if self.nodes[node].pattern {
                crate::contract::azure::pattern(&block.ty, name)
            } else {
                entry.project(name)
            },
            pattern: self.nodes[node].pattern,
            through: self.nodes[node].overridable.clone(),
        };
        self.nodes[node].provisioned = Some(projection);
    }
    pub(super) fn azure_workload(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) {
        if block.kind != Kind::Resource
            || self.work == 0
            || self.root_left == 0
            || !matches!(
                block.ty.as_str(),
                "azurerm_linux_function_app" | "azurerm_windows_function_app"
            )
        {
            return;
        }
        let mut candidate = Node::default();
        if block.ty == "azurerm_linux_function_app" {
            let dockers = block
                .nested
                .iter()
                .filter(|b| b.ident.as_str() == "site_config")
                .flat_map(|b| b.body.blocks())
                .filter(|b| b.ident.as_str() == "application_stack")
                .flat_map(|b| b.body.blocks())
                .filter(|b| b.ident.as_str() == "docker")
                .collect::<Vec<_>>();
            if let [docker] = dockers.as_slice() {
                let mut through = BTreeSet::new();
                let mut fields = Vec::new();
                for field in ["registry_url", "image_name", "image_tag"] {
                    let Some(attr) = docker.body.get_attribute(field) else {
                        break;
                    };
                    let evaluated = self.value(at, each, &attr.value, 0);
                    let (text, pattern) = evaluated.text();
                    if text.is_empty()
                        || (field == "registry_url" && pattern)
                        || (field == "image_name" && text == "*")
                    {
                        break;
                    }
                    through.extend(evaluated.through);
                    fields.push(text);
                }
                if let [registry, name, tag] = fields.as_slice() {
                    let registry = registry
                        .strip_prefix("https://")
                        .unwrap_or(registry)
                        .trim_end_matches('/');
                    if !registry.is_empty() && !registry.contains(['/', '?', '#', '@', '$']) {
                        let bytes = block.name.len()
                            + registry.len()
                            + name.len()
                            + tag.len()
                            + 2
                            + through
                                .iter()
                                .map(|o| o.path.len() + o.variable.len() + 16)
                                .sum::<usize>();
                        if self.charge(bytes) {
                            candidate.images.push(WorkloadImage {
                                container: block.name.clone(),
                                image: Resolved {
                                    text: format!("{registry}/{name}:{tag}"),
                                    overridable: !through.is_empty(),
                                },
                                through: through.into_iter().collect(),
                            });
                        }
                    }
                }
            }
        }
        if let Some(arg) = block.args.get("zip_deploy_file") {
            let evaluated = self.value(at, each, &arg.expr, 0);
            if let Val::Known(Value::String(path)) = evaluated.val {
                let base = if self.root == "." {
                    ""
                } else {
                    self.root.as_str()
                };
                if let Some(text) = workload::local_path(base, &path)
                    && self.charge(text.len())
                {
                    candidate.build = Some(Resolved {
                        text,
                        overridable: !evaluated.through.is_empty(),
                    });
                }
            }
        }
        if let Some(arg) = block.args.get("app_settings") {
            let mut bytes = 0usize;
            let mut entries = BTreeMap::new();
            let mut put = |eval: &mut Self,
                           name: &str,
                           (mut target, mut value, mut contract): (
                String,
                Option<String>,
                Option<String>,
            )| {
                if let Some(reference) = value
                    .as_deref()
                    .and_then(crate::contract::azure::keyvault_reference)
                {
                    target = reference;
                    value = None;
                    contract = None;
                }
                let needed = block.name.len()
                    + name.len()
                    + target.len()
                    + value.as_ref().map_or(0, String::len)
                    + contract.as_ref().map_or(0, String::len)
                    + candidate.images.first().map_or(0, |i| i.image.text.len());
                bytes = bytes.saturating_add(needed);
                if bytes > environment::ENVIRONMENT_BYTES_MAX || !eval.charge(needed) {
                    entries.clear();
                    return false;
                }
                entries.insert(
                    name.to_string(),
                    environment::Entry {
                        container: block.name.clone(),
                        image: candidate.images.first().map(|i| i.image.text.clone()),
                        name: name.into(),
                        target,
                        value,
                        contract,
                    },
                );
                true
            };
            if let Expression::Object(settings) = &arg.expr {
                for (key, value) in settings.iter() {
                    let name = match key {
                        ObjectKey::Ident(i) => i.as_str(),
                        ObjectKey::Expression(Expression::String(s)) => s.value(),
                        _ => {
                            entries.clear();
                            break;
                        }
                    };
                    let found = self.tf_environment(at, each, value.expr());
                    if !put(self, name, found) {
                        break;
                    }
                }
            } else if let Val::Known(Value::Object(settings)) =
                self.value(at, each, &arg.expr, 0).val
            {
                for (name, value) in settings {
                    if !put(
                        self,
                        &name,
                        (
                            "Terraform environment".into(),
                            workload::literal(value),
                            None,
                        ),
                    ) {
                        break;
                    }
                }
            }
            candidate.environment = entries.into_values().collect();
        }
        self.nodes[node].images = candidate.images;
        self.nodes[node].build = candidate.build;
        self.nodes[node].environment = candidate.environment;
    }

    /// Qualify subscription identities with the complete, evaluated topic before provision/link.
    pub(super) fn azure_subscriptions(
        &mut self,
        pending: &[(usize, usize, &'m Block, Option<Each>)],
    ) {
        for &(node, at, block, ref each) in pending {
            if block.kind != Kind::Resource || block.ty != "azurerm_servicebus_subscription" {
                continue;
            }
            let topic = self.endpoint_arg(
                at,
                each.as_ref(),
                block,
                "topic_id",
                "azurerm_servicebus_topic",
            );
            let (Some(name), Some(topic)) = (self.nodes[node].name.as_deref(), topic) else {
                continue;
            };
            let bytes = "subscription:".len()
                + topic.name.len()
                + 1
                + name.len()
                + self.nodes[node]
                    .overridable
                    .iter()
                    .chain(topic.through.iter())
                    .map(|o| o.path.len() + o.variable.len() + 16)
                    .sum::<usize>();
            if !self.charge(bytes) {
                return;
            }
            let name = self.nodes[node].name.as_deref().unwrap();
            let pattern = self.nodes[node].pattern;
            let target = if pattern {
                crate::contract::azure::pattern(&block.ty, name)
                    .map(|name| format!("subscription:{}/{name}", topic.name))
            } else {
                crate::contract::azure::subscription_target(&topic.name, name)
            };
            let through: BTreeSet<_> = self.nodes[node]
                .overridable
                .iter()
                .cloned()
                .chain(topic.through)
                .collect();
            self.nodes[node].provisioned = Some(Projection {
                name: target,
                pattern,
                through: through.into_iter().collect(),
            });
        }
        for &(rule, at, block, ref each) in pending {
            if block.kind != Kind::Resource || block.ty != "azurerm_servicebus_subscription_rule" {
                continue;
            }
            let Some(subscription) = self.endpoint_arg(
                at,
                each.as_ref(),
                block,
                "subscription_id",
                "azurerm_servicebus_subscription",
            ) else {
                continue;
            };
            if !self.charge(subscription.name.len() + self.nodes[rule].address.len()) {
                return;
            }
            self.subscription_inputs
                .azure_filters
                .entry(subscription.name)
                .or_insert_with(|| self.nodes[rule].address.clone());
        }
    }
}
