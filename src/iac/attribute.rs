//! Exact resource attributes share the evaluator's aliases, instance selection and budgets.

use super::*;

pub(super) struct At<'m> {
    pub at: usize,
    pub each: Option<Each>,
    pub expr: &'m Expression,
    pub through: BTreeSet<Overridable>,
}

pub(super) struct Attribute {
    pub node: usize,
    pub name: String,
    pub through: BTreeSet<Overridable>,
}

impl<'m> Eval<'m> {
    pub(super) fn instance_through(&self, mut at: usize) -> BTreeSet<Overridable> {
        let mut through = BTreeSet::new();
        while let Some((parent, _)) = self.instances[at].parent {
            if let Some(each) = &self.instances[at].call {
                through.extend(each.through.iter().cloned());
            }
            at = parent;
        }
        through
    }
    /// A collapsed unknown module expansion is a declaration, never one selected instance.
    pub(super) fn bound_instance(&self, mut at: usize) -> bool {
        while let Some((parent, block)) = self.instances[at].parent {
            if self.instances[at].call.is_none()
                && (block.count.is_some() || block.for_each.is_some())
            {
                return false;
            }
            at = parent;
        }
        true
    }
    fn selected_reference(
        &mut self,
        at: usize,
        each: Option<&Each>,
        traversal: &'m Traversal,
        depth: usize,
    ) -> Option<(Ref, BTreeSet<Overridable>)> {
        let Expression::Variable(root) = &traversal.expr else {
            return None;
        };
        let ops: Vec<_> = traversal.operators.iter().map(|o| o.value()).collect();
        let mut reference = reference(root.as_str(), &ops)?;
        let mut through = BTreeSet::new();
        if let Ref::Block {
            key,
            kind: Kind::Resource | Kind::Module,
            ..
        } = &mut reference
        {
            match ops.get(1) {
                Some(TraversalOperator::Index(expr)) => {
                    let selected = self.value(at, each, expr, depth);
                    through.extend(selected.through);
                    *key = Some(match selected.val {
                        Val::Known(Value::String(s)) => Key::Str(s),
                        Val::Known(Value::Number(n)) => Key::Int(n.as_u64()?),
                        _ => return None,
                    });
                }
                Some(TraversalOperator::AttrSplat(_) | TraversalOperator::FullSplat(_)) => {
                    return None;
                }
                _ => {}
            }
        }
        Some((reference, through))
    }

    /// Follow expression aliases, never arbitrary resource contents or sensitive variables.
    pub(super) fn unalias(
        &mut self,
        at: usize,
        each: Option<&Each>,
        expr: &'m Expression,
        depth: usize,
    ) -> Option<At<'m>> {
        if depth > EVAL_DEPTH_MAX {
            return None;
        }
        self.work = self.work.checked_sub(1)?;
        if let Expression::Parenthesis(p) = expr {
            return self.unalias(at, each, p.inner(), depth + 1);
        }
        let Expression::Traversal(t) = expr else {
            return Some(At {
                at,
                each: each.cloned(),
                expr,
                through: BTreeSet::new(),
            });
        };
        let (reference, through) = self.selected_reference(at, each, t, depth)?;
        let ops = t.operators.len();
        let modules = self.modules;
        let mut found = match reference {
            Ref::Local(n) if ops == 1 => self.unalias(
                at,
                each,
                &modules[self.instances[at].dir].locals.get(&n)?.expr,
                depth + 1,
            )?,
            Ref::Var(n) if ops == 1 && self.instances[at].parent.is_some() => {
                let instance = &self.instances[at];
                if modules[instance.dir].variables.get(&n)?.sensitive {
                    return None;
                }
                let (parent, call) = instance.parent?;
                let call_each = instance.call.clone();
                let Some(argument) = call.args.get(&n) else {
                    return Some(At {
                        at,
                        each: each.cloned(),
                        expr,
                        through,
                    });
                };
                self.unalias(parent, call_each.as_ref(), &argument.expr, depth + 1)?
            }
            Ref::Block {
                kind: Kind::Module,
                name,
                key,
                output: Some(output),
                ..
            } if ops == 2 + usize::from(key.is_some()) => {
                let children = self.instances[at].children.get(name.as_str())?;
                let mut matches = children.iter().filter(|(k, _)| k == &key);
                let (_, child) = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                let child = *child;
                if !self.bound_instance(child) {
                    return None;
                }
                let mut found = self.unalias(
                    child,
                    None,
                    &modules[self.instances[child].dir]
                        .outputs
                        .get(&output)?
                        .expr,
                    depth + 1,
                )?;
                found.through.extend(self.instance_through(child));
                found
            }
            _ => {
                return Some(At {
                    at,
                    each: each.cloned(),
                    expr,
                    through,
                });
            }
        };
        found.through.extend(through);
        Some(found)
    }

    pub(super) fn resource_attribute(
        &mut self,
        at: usize,
        each: Option<&Each>,
        expr: &'m Expression,
        depth: usize,
    ) -> Option<Attribute> {
        let found = self.unalias(at, each, expr, depth)?;
        let Expression::Traversal(t) = found.expr else {
            return None;
        };
        let (reference, mut through) =
            self.selected_reference(found.at, found.each.as_ref(), t, depth)?;
        let Ref::Block {
            kind: Kind::Resource,
            ty,
            name,
            key,
            ..
        } = reference
        else {
            return None;
        };
        let ops: Vec<_> = t.operators.iter().map(|o| o.value()).collect();
        if ops.len() != 2 + usize::from(key.is_some()) {
            return None;
        }
        let TraversalOperator::GetAttr(attribute) = ops.last()? else {
            return None;
        };
        let instances =
            self.instances[found.at]
                .nodes
                .get(&(Kind::Resource, ty.as_str(), name.as_str()))?;
        let mut matches = instances.iter().filter(|(k, _)| k == &key);
        let (_, address) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        through.extend(found.through);
        let (node, selected) = self.node_inputs.get(address)?;
        through.extend(selected.iter().cloned());
        Some(Attribute {
            node: *node,
            name: attribute.as_str().into(),
            through,
        })
    }
}
