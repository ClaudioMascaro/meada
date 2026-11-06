//! GCP Terraform projections, evaluated in the existing root and instance budget.
use super::environment::Entry;
use super::*;

impl<'m> Eval<'m> {
    /// Empty nested blocks still cost work. Charge scans before collecting their children.
    fn gcp_walk(&mut self, structures: usize) -> bool {
        let cost = structures
            .max(1)
            .saturating_mul(std::mem::size_of::<hcl_edit::structure::Block>());
        self.work = self.work.saturating_sub(cost);
        self.work != 0
    }

    pub(super) fn gcp_projection(&mut self, node: usize, block: &'m Block) {
        if block.kind != Kind::Resource
            || !block.ty.starts_with("google_")
            || block.ty == "google_sql_database"
        {
            return;
        }
        let Some(entry) = types::entry("terraform", &block.ty) else {
            return;
        };
        let Some(text) = self.nodes[node].name.as_deref() else {
            return;
        };
        // Charge the largest possible projection before allocating its name/provenance.
        // Structured identifiers may shrink; only a subscription adds a fixed prefix.
        let bytes = text.len()
            + if block.ty == "google_pubsub_subscription" {
                "subscriptions/".len()
            } else {
                0
            }
            + self.nodes[node]
                .overridable
                .iter()
                .map(|o| o.path.len() + o.variable.len() + 16)
                .sum::<usize>();
        if !self.charge(bytes) {
            return;
        }
        let text = self.nodes[node].name.as_deref().unwrap();
        let pattern = self.nodes[node].pattern;
        let name = if pattern {
            crate::contract::gcp::pattern(&block.ty, text)
        } else {
            entry.project(text)
        };
        self.nodes[node].provisioned = Some(Projection {
            name,
            pattern,
            through: self.nodes[node].overridable.clone(),
        });
    }

    pub(super) fn gcp_workload(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) {
        if block.kind != Kind::Resource || self.work == 0 || self.root_left == 0 {
            return;
        }
        let path: &[&str] = match block.ty.as_str() {
            "google_cloud_run_service" => &["template", "spec", "containers"],
            "google_cloud_run_v2_service" => &["template", "containers"],
            "google_cloud_run_v2_job" => &["template", "template", "containers"],
            _ => return,
        };
        if !self.gcp_walk(block.nested.len()) {
            return;
        }
        let mut containers = block
            .nested
            .iter()
            .filter(|b| b.ident.as_str() == path[0])
            .collect::<Vec<_>>();
        for part in &path[1..] {
            let mut children = Vec::new();
            for parent in containers {
                if !self.gcp_walk(parent.body.len()) {
                    return;
                }
                children.extend(parent.body.blocks().filter(|b| b.ident.as_str() == *part));
            }
            containers = children;
        }
        let mut environment_bytes = 0usize;
        let mut environment_full = false;
        for (index, container) in containers.into_iter().enumerate() {
            if !self.gcp_walk(container.body.len()) {
                return;
            }
            let name = container
                .body
                .get_attribute("name")
                .and_then(|a| match self.value(at, each, &a.value, 0).val {
                    Val::Known(Value::String(name)) if !name.is_empty() => Some(name),
                    _ => None,
                })
                .unwrap_or_else(|| format!("container[{index}]"));
            let image = container.body.get_attribute("image").and_then(|a| {
                let found = self.value(at, each, &a.value, 0);
                let (text, _) = found.text();
                if text == "*" {
                    return None;
                }
                let bytes = name.len()
                    + text.len()
                    + found
                        .through
                        .iter()
                        .map(|o| o.path.len() + o.variable.len() + 16)
                        .sum::<usize>();
                if !self.charge(bytes) {
                    return None;
                }
                Some(WorkloadImage {
                    container: name.clone(),
                    image: Resolved {
                        text,
                        overridable: !found.through.is_empty(),
                    },
                    through: found.through.into_iter().collect(),
                })
            });
            if image.is_none() && self.root_left == 0 {
                return;
            }
            if environment_full {
                self.nodes[node].images.extend(image);
                continue;
            }
            let mut entries = BTreeMap::<String, Entry>::new();
            for env in container
                .body
                .blocks()
                .filter(|b| b.ident.as_str() == "env")
            {
                if !self.gcp_walk(env.body.len()) {
                    self.nodes[node].images.extend(image);
                    return;
                }
                let Some(arg) = env.body.get_attribute("name") else {
                    continue;
                };
                let Val::Known(Value::String(variable)) = self.value(at, each, &arg.value, 0).val
                else {
                    continue;
                };
                if variable.is_empty() || variable.len() > crate::config_values::VALUE_MAX {
                    continue;
                }
                let sources = env
                    .body
                    .blocks()
                    .filter(|b| matches!(b.ident.as_str(), "value_from" | "value_source"))
                    .collect::<Vec<_>>();
                let (target, value, contract) =
                    match (env.body.get_attribute("value"), sources.as_slice()) {
                        (Some(value), []) => self.tf_environment(at, each, &value.value),
                        (None, [source]) => (
                            format!("secret {}", self.gcp_secret_target(at, each, source)),
                            None,
                            None,
                        ),
                        _ => ("unresolved environment".into(), None, None),
                    };
                let raw_image = image.as_ref().map(|image| image.image.text.as_str());
                let bytes = name.len()
                    + raw_image.map_or(0, str::len)
                    + variable.len()
                    + target.len()
                    + value.as_ref().map_or(0, String::len)
                    + contract.as_ref().map_or(0, String::len);
                environment_bytes = environment_bytes.saturating_add(bytes);
                if environment_bytes > environment::ENVIRONMENT_BYTES_MAX {
                    self.nodes[node].environment.clear();
                    entries.clear();
                    environment_full = true;
                    break;
                }
                if !self.charge(bytes) {
                    self.nodes[node].images.extend(image);
                    return;
                }
                let entry = Entry {
                    container: name.clone(),
                    image: raw_image.map(str::to_string),
                    name: variable.clone(),
                    target,
                    value,
                    contract,
                };
                entries
                    .entry(variable)
                    .and_modify(|old| {
                        old.target = "duplicate environment entry".into();
                        old.value = None;
                        old.contract = None;
                    })
                    .or_insert(entry);
            }
            self.nodes[node].environment.extend(entries.into_values());
            self.nodes[node].images.extend(image);
        }
    }

    fn gcp_secret_target(
        &mut self,
        at: usize,
        each: Option<&Each>,
        source: &'m hcl_edit::structure::Block,
    ) -> String {
        if !self.gcp_walk(source.body.len()) {
            return "unresolved".into();
        }
        let mut refs = source
            .body
            .blocks()
            .filter(|b| b.ident.as_str() == "secret_key_ref");
        let Some(secret) = refs.next() else {
            return "unresolved".into();
        };
        if refs.next().is_some() || !self.gcp_walk(secret.body.len()) {
            return "unresolved".into();
        }
        let name = secret
            .body
            .get_attribute("secret")
            .or_else(|| secret.body.get_attribute("name"));
        name.map(|arg| {
            let (target, value, contract) = self.tf_environment(at, each, &arg.value);
            value.or(contract).unwrap_or(target)
        })
        .unwrap_or_else(|| "unresolved".into())
    }

    pub(super) fn gcp_database(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) {
        if block.kind != Kind::Resource || block.ty != "google_sql_database" {
            return;
        }
        // A database with an unreadable instance must never fall back to its short name.
        self.nodes[node].provisioned = Some(Projection::default());
        let Some(instance) =
            self.endpoint_arg(at, each, block, "instance", "google_sql_database_instance")
        else {
            return;
        };
        let Some(database) = self.nodes[node].name.as_deref() else {
            return;
        };
        let pattern = self.nodes[node].pattern;
        if !(crate::contract::gcp::database_name(database)
            || (pattern && super::pattern::supported(database)))
        {
            return;
        }
        let bytes = instance.name.len()
            + 1
            + database.len()
            + self.nodes[node]
                .overridable
                .iter()
                .chain(instance.through.iter())
                .map(|o| o.path.len() + o.variable.len() + 16)
                .sum::<usize>();
        if !self.charge(bytes) {
            return;
        }
        let name = format!(
            "{}/{}",
            instance.name,
            self.nodes[node].name.as_deref().unwrap()
        );
        let through: BTreeSet<_> = self.nodes[node]
            .overridable
            .iter()
            .cloned()
            .chain(instance.through)
            .collect();
        self.nodes[node].provisioned = Some(Projection {
            name: Some(name),
            pattern,
            through: through.into_iter().collect(),
        });
    }
}
