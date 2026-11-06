//! Module input names use the ordinary Terraform evaluator, in the call's own instance.

use super::super::*;

impl<'m> Eval<'m> {
    pub(in crate::iac) fn mapped_module(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) {
        if block.kind != Kind::Module {
            return;
        }
        let Some(mapping) = block
            .source
            .as_deref()
            .and_then(|s| self.mappings.module(s))
        else {
            return;
        };
        let entry = mapping.entry.clone();
        let input = mapping.input.clone();
        let projection = Projection {
            entry: Some(entry.clone()),
            ..Default::default()
        };
        self.nodes[node].provisioned = Some(projection);
        let Some(steps) = crate::config_values::sites::parts_without_selectors(&input) else {
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
        let (name, pattern) = found.text();
        if !self.charge(
            name.len()
                + entry.len()
                + input.len()
                + block.json_path.as_ref().map_or(0, String::len),
        ) {
            return;
        }
        self.nodes[node].name = Some(name.clone());
        self.nodes[node].pattern = pattern;
        self.nodes[node].overridable = found.through.iter().cloned().collect();
        let mut through = found.through;
        if let Some(each) = each {
            through.extend(each.through.iter().cloned());
        }
        let created = match block.args.get("create") {
            None => true,
            Some(arg) => {
                let value = self.value(at, each, &arg.expr, 0);
                through.extend(value.through);
                matches!(value.val, Val::Known(Value::Bool(true)))
            }
        };
        let expanded = [block.count.as_ref(), block.for_each.as_ref()]
            .into_iter()
            .flatten()
            .all(|expr| {
                let value = self.value(at, each, expr, 0);
                through.extend(value.through);
                matches!(value.val, Val::Known(_))
            });
        let target = created
            .then(|| types::named(&entry)?.mapped(&name, pattern))
            .flatten()
            .filter(|_| expanded);
        let block_key = block
            .json_path
            .clone()
            .unwrap_or_else(|| format!("module.{}", block.name));
        let projection = Projection {
            name: target,
            pattern,
            through: through.into_iter().collect(),
            entry: Some(entry),
            defining: vec![format!("{block_key}.{input}")],
        };
        if self.charge(projection.size()) {
            self.nodes[node].provisioned = Some(projection);
        }
    }
}
