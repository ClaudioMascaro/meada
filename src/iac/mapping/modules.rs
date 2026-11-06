//! Module inputs use the ordinary Terraform evaluator, in the call's own instance.

use super::super::*;

struct Spec {
    entry: String,
    input: String,
    create: Vec<String>,
    disable: Vec<String>,
    registry: Option<&'static types::Registry>,
}

impl<'m> Eval<'m> {
    fn module_spec(&self, block: &Block) -> Option<Spec> {
        if block.kind != Kind::Module {
            return None;
        }
        let source = block
            .source
            .as_deref()
            .filter(|s| !s.contains(['$', '{', '}']))?;
        if let Some(mapping) = self.mappings.module(source) {
            return Some(Spec {
                entry: mapping.entry.clone(),
                input: mapping.input.clone(),
                create: vec!["create".into()],
                disable: Vec::new(),
                registry: None,
            });
        }
        let registry = types::registry(source)?;
        Some(Spec {
            entry: registry.entry.clone(),
            input: registry.input.clone(),
            create: registry.create.clone(),
            disable: registry.disable.clone(),
            registry: Some(registry),
        })
    }

    pub(in crate::iac) fn module_bool(
        &mut self,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
        key: &str,
        default: bool,
        through: &mut BTreeSet<Overridable>,
    ) -> Option<bool> {
        let Some(arg) = block.args.get(key) else {
            return Some(default);
        };
        let value = self.value(at, each, &arg.expr, 0);
        through.extend(value.through);
        match value.val {
            Val::Known(Value::Bool(value)) => Some(value),
            _ => None,
        }
    }

    fn module_created(
        &mut self,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
        spec: &Spec,
    ) -> (bool, BTreeSet<Overridable>) {
        let mut through = BTreeSet::new();
        if let Some(each) = each {
            through.extend(each.through.iter().cloned());
        }
        let mut created = true;
        for key in &spec.create {
            created &= self.module_bool(at, each, block, key, true, &mut through) == Some(true);
        }
        for key in &spec.disable {
            created &= self.module_bool(at, each, block, key, false, &mut through) == Some(false);
        }
        if let Some(expr) = &block.count {
            let value = self.value(at, each, expr, 0);
            through.extend(value.through);
            created &= matches!(value.val, Val::Known(Value::Number(n)) if n.as_i64().is_some_and(|n| n > 0));
        }
        if let Some(expr) = &block.for_each {
            let value = self.value(at, each, expr, 0);
            through.extend(value.through);
            created &= matches!(value.val, Val::Known(Value::Array(_) | Value::Object(_)));
        }
        (created, through)
    }

    pub(in crate::iac) fn mapped_module(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) {
        let Some(spec) = self.module_spec(block) else {
            return;
        };
        self.nodes[node].provisioned = Some(Projection {
            entry: Some(spec.entry.clone()),
            ..Default::default()
        });
        let Some(steps) = crate::config_values::sites::parts_without_selectors(&spec.input) else {
            return;
        };
        let Some((Step::Key(head), tail)) = steps.split_first() else {
            return;
        };
        let Some(arg) = block.args.get(head) else {
            return;
        };
        let mut found = self.value(at, each, &arg.expr, 0);
        for step in tail {
            let Val::Known(value) = &found.val else {
                return;
            };
            let selected = match (step, value) {
                (Step::Key(k), Value::Object(object)) => object.get(k),
                (Step::Index(i), Value::Array(array)) => array.get(*i),
                _ => None,
            };
            let Some(selected) = selected else { return };
            found.val = Val::Known(selected.clone());
        }
        if matches!(
            found.val,
            Val::Known(Value::Null | Value::Array(_) | Value::Object(_))
        ) {
            return;
        }
        let (mut name, mut pattern) = found.text();
        let mut generated = false;
        if let Some(registry) = spec.registry {
            if let Some(flag) = &registry.fifo {
                match self.module_bool(at, each, block, flag, false, &mut found.through) {
                    Some(fifo) => {
                        name = name.strip_suffix(".fifo").unwrap_or(&name).to_string();
                        if fifo {
                            name.push_str(".fifo");
                        }
                    }
                    None => {
                        name = "*".into();
                        pattern = true;
                    }
                }
            }
            if let Some(flag) = &registry.prefix {
                match self.module_bool(at, each, block, flag, false, &mut found.through) {
                    Some(true) => {
                        name.push_str("-*");
                        pattern = true;
                        generated = true;
                    }
                    Some(false) => {}
                    None => {
                        name = "*".into();
                        pattern = true;
                        generated = true;
                    }
                }
            }
        }
        if !self.charge(
            name.len()
                + spec.entry.len()
                + spec.input.len()
                + block.json_path.as_ref().map_or(0, String::len),
        ) {
            return;
        }
        self.nodes[node].name = Some(name.clone());
        self.nodes[node].pattern = pattern;
        self.nodes[node].overridable = found.through.iter().cloned().collect();
        let (created, gate_through) = self.module_created(at, each, block, &spec);
        found.through.extend(gate_through);
        let target = (created && !generated)
            .then(|| types::named(&spec.entry)?.mapped(&name, pattern))
            .flatten();
        let block_key = block
            .json_path
            .clone()
            .unwrap_or_else(|| format!("module.{}", block.name));
        let dot = if spec.input.starts_with('[') { "" } else { "." };
        let projection = Projection {
            name: target,
            pattern,
            through: found.through.into_iter().collect(),
            entry: Some(spec.entry),
            defining: vec![format!("{block_key}{dot}{}", spec.input)],
            ..Default::default()
        };
        if self.charge(projection.size()) {
            self.nodes[node].provisioned = Some(projection);
        }
    }

    /// Workloads wait until the shared endpoint indexes and cloud projections are prepared.
    pub(in crate::iac) fn module_workload(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) {
        if self.nodes[node].name.is_none() {
            return;
        }
        let Some(spec) = self.module_spec(block) else {
            return;
        };
        let Some(workload) = types::named_workload(&spec.entry) else {
            return;
        };
        let (created, through) = self.module_created(at, each, block, &spec);
        if !created {
            return;
        }
        match workload.ty.as_str() {
            "aws_lambda_function" => self.module_lambda(node, at, each, block, &through),
            "aws_ecs_task_definition" => self.ecs_module(node, at, each, block, &through),
            _ => {}
        }
    }
}
