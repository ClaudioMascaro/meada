//! Cloud subscription summaries: evaluated in their root and instance, then routed by
//! the channel kind. Resource attributes name only a known node, never a cloud API value.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Subscription {
    pub source: String,
    pub node: String,
    pub root: String,
    pub target: String,
    pub inferred: bool,
    pub reason: String,
}

pub(crate) fn reads(ty: &str) -> bool {
    matches!(
        ty,
        "aws_sns_topic_subscription"
            | "aws_cloudwatch_event_rule"
            | "aws_cloudwatch_event_target"
            | "azurerm_servicebus_subscription"
            | "azurerm_servicebus_subscription_rule"
            | "google_pubsub_subscription"
    )
}

pub(crate) fn read(db: &Connection, identity: &str) -> Result<Vec<Subscription>> {
    let mut statement = db.prepare(&format!(
        "SELECT {COLUMNS} FROM iac_nodes WHERE subscription IS NOT NULL"
    ))?;
    let mut out = Vec::new();
    for node in statement.query_map([], row)? {
        let node = node?;
        if let Some(mut s) = node.subscription {
            s.node = format!("{identity}::{}::{}", node.path, node.address);
            out.push(s);
        }
    }
    out.sort_by(|a, b| (&a.source, &a.target, &a.reason).cmp(&(&b.source, &b.target, &b.reason)));
    Ok(out)
}

#[derive(Clone)]
pub(super) struct Endpoint {
    pub(super) name: String,
    pub(super) through: BTreeSet<Overridable>,
}

#[derive(Clone)]
struct Rule {
    node: usize,
    bus: Endpoint,
    filtered: bool,
}

/// Root-local indexes; duplicate physical rules on the same bus remain ambiguous.
#[derive(Default)]
pub(super) struct Inputs {
    nodes: HashMap<String, usize>,
    rules: HashMap<(String, String), Option<Rule>>,
    pub(super) azure_filters: HashMap<String, String>,
}

impl<'m> Eval<'m> {
    pub(super) fn prepare_subscriptions(
        &mut self,
        pending: &[(usize, usize, &'m Block, Option<Each>)],
    ) {
        for &(node, _, _, _) in pending {
            self.subscription_inputs
                .nodes
                .entry(self.nodes[node].address.clone())
                .or_insert(node);
        }
        for &(node, instance, block, ref each) in pending {
            if block.kind != Kind::Resource
                || block.ty != "aws_cloudwatch_event_rule"
                || (block.args.contains_key("schedule_expression")
                    && !block.args.contains_key("event_pattern"))
            {
                continue;
            }
            let Some(name) = self.nodes[node].name.clone() else {
                continue;
            };
            let Some(mut bus) = self.bus(instance, each.as_ref(), block) else {
                continue;
            };
            bus.through
                .extend(self.nodes[node].overridable.iter().cloned());
            let key = (name, bus.name.clone());
            self.subscription_inputs
                .rules
                .entry(key)
                .and_modify(|found| *found = None)
                .or_insert(Some(Rule {
                    node,
                    bus,
                    filtered: block.args.contains_key("event_pattern"),
                }));
        }
    }

    /// Resolve literals with the ordinary evaluator, and a traversal to a known resource's
    /// name/ARN/id with its node. Other attributes, unknown expressions and ambiguous instances
    /// remain absent. Following aliases shares the evaluator's depth and work budgets.
    pub(super) fn endpoint(
        &mut self,
        at: usize,
        each: Option<&Each>,
        expr: &'m Expression,
        ty: &str,
        depth: usize,
    ) -> Option<Endpoint> {
        if depth > EVAL_DEPTH_MAX || self.work == 0 {
            return None;
        }
        let found = self.value(at, each, expr, depth);
        if let Val::Known(Value::String(raw)) = found.val {
            let name = match types::entry("terraform", ty) {
                Some(entry) => entry.project(&raw)?,
                None => raw,
            };
            if name.is_empty()
                || name.starts_with("arn:")
                || name.starts_with("https:")
                || name.contains('*')
            {
                return None;
            }
            return Some(Endpoint {
                name,
                through: found.through,
            });
        }
        let t = match expr {
            Expression::Parenthesis(p) => return self.endpoint(at, each, p.inner(), ty, depth + 1),
            Expression::Traversal(t) => t,
            _ => return None,
        };
        let Expression::Variable(root) = &t.expr else {
            return None;
        };
        let ops: Vec<_> = t.operators.iter().map(|o| o.value()).collect();
        let mut r = reference(root.as_str(), &ops)?;
        let mut selected_through = BTreeSet::new();
        if let Ref::Block {
            key,
            kind: Kind::Resource | Kind::Module,
            ..
        } = &mut r
        {
            match ops.get(1) {
                Some(TraversalOperator::Index(expr)) => {
                    let selected = self.value(at, each, expr, depth);
                    selected_through.extend(selected.through);
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
        let modules = self.modules;
        let mut endpoint = match r {
            Ref::Local(n) if ops.len() == 1 => self.endpoint(
                at,
                each,
                &modules[self.instances[at].dir].locals.get(&n)?.expr,
                ty,
                depth + 1,
            ),
            Ref::Var(n) if ops.len() == 1 => {
                // Root defaults were evaluated above. Only a local module's argument may
                // carry a resource reference, and a sensitive variable is never followed.
                let instance = &self.instances[at];
                if modules[instance.dir].variables.get(&n)?.sensitive {
                    return None;
                }
                let (parent, call) = instance.parent?;
                let arg = &call.args.get(&n)?.expr;
                let call_each = instance.call.clone();
                self.endpoint(parent, call_each.as_ref(), arg, ty, depth + 1)
            }
            Ref::Block {
                kind: Kind::Module,
                name,
                key,
                output: Some(out),
                ..
            } if ops.len() == 2 + usize::from(key.is_some()) => {
                let children = self.instances[at].children.get(name.as_str())?;
                let matches: Vec<_> = children.iter().filter(|(k, _)| k == &key).collect();
                let [(_, child)] = matches[..] else {
                    return None;
                };
                let child = *child;
                self.endpoint(
                    child,
                    None,
                    &modules[self.instances[child].dir].outputs.get(&out)?.expr,
                    ty,
                    depth + 1,
                )
            }
            Ref::Block {
                kind: Kind::Resource,
                ty: kind,
                name,
                key,
                ..
            } if kind == ty => {
                if ops.len() != 2 + usize::from(key.is_some())
                    || !matches!(ops.last(), Some(TraversalOperator::GetAttr(a)) if matches!(a.as_str(), "id" | "name") || (a.as_str() == "arn" && ty.starts_with("aws_")) || (a.as_str() == "secret_id" && ty == "google_secret_manager_secret"))
                {
                    return None;
                }
                let instances =
                    self.instances[at]
                        .nodes
                        .get(&(Kind::Resource, ty, name.as_str()))?;
                let matches: Vec<_> = instances.iter().filter(|(k, _)| k == &key).collect();
                let [(_, address)] = matches[..] else {
                    return None;
                };
                let node = &self.nodes[*self.subscription_inputs.nodes.get(address)?];
                if node
                    .provisioned
                    .as_ref()
                    .map_or(node.pattern, |p| p.pattern)
                {
                    return None;
                }
                Some(Endpoint {
                    name: node
                        .provisioned
                        .as_ref()
                        .map_or(node.name.as_ref(), |p| p.name.as_ref())?
                        .clone(),
                    through: node
                        .provisioned
                        .as_ref()
                        .map_or(&node.overridable, |p| &p.through)
                        .iter()
                        .cloned()
                        .collect(),
                })
            }
            _ => None,
        }?;
        endpoint.through.extend(selected_through);
        Some(endpoint)
    }

    pub(super) fn endpoint_arg(
        &mut self,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
        arg: &str,
        ty: &str,
    ) -> Option<Endpoint> {
        self.endpoint(at, each, &block.args.get(arg)?.expr, ty, 0)
    }

    fn bus(&mut self, at: usize, each: Option<&Each>, block: &'m Block) -> Option<Endpoint> {
        if block.args.contains_key("event_bus_name") {
            self.endpoint_arg(
                at,
                each,
                block,
                "event_bus_name",
                "aws_cloudwatch_event_bus",
            )
        } else {
            Some(Endpoint {
                name: "default".into(),
                through: BTreeSet::new(),
            })
        }
    }

    pub(super) fn subscription(
        &mut self,
        node: usize,
        at: usize,
        each: Option<&Each>,
        block: &'m Block,
    ) -> Option<Subscription> {
        if block.kind != Kind::Resource {
            return None;
        }
        let (source, target, filtered, via) = match block.ty.as_str() {
            "azurerm_servicebus_subscription" => {
                let source =
                    self.endpoint_arg(at, each, block, "topic_id", "azurerm_servicebus_topic")?;
                let projected = self.nodes[node].provisioned.as_ref()?;
                if projected.pattern {
                    return None;
                }
                let target = Endpoint {
                    name: projected.name.clone()?,
                    through: projected.through.iter().cloned().collect(),
                };
                let rule = self
                    .subscription_inputs
                    .azure_filters
                    .get(&target.name)
                    .cloned();
                (source, target, rule.is_some(), rule)
            }
            "aws_sns_topic_subscription" => {
                let protocol = self.value(at, each, &block.args.get("protocol")?.expr, 0);
                if protocol.val != Val::Known(Value::String("sqs".into())) {
                    return None;
                }
                (
                    self.endpoint_arg(at, each, block, "topic_arn", "aws_sns_topic")?,
                    self.endpoint_arg(at, each, block, "endpoint", "aws_sqs_queue")?,
                    block.args.contains_key("filter_policy"),
                    None,
                )
            }
            "aws_cloudwatch_event_target" => {
                let rule =
                    self.endpoint_arg(at, each, block, "rule", "aws_cloudwatch_event_rule")?;
                let mut bus = self.bus(at, each, block)?;
                bus.through.extend(rule.through);
                let matched = self
                    .subscription_inputs
                    .rules
                    .get(&(rule.name, bus.name.clone()))?
                    .as_ref()?
                    .clone();
                bus.through.extend(matched.bus.through);
                (
                    bus,
                    self.endpoint_arg(at, each, block, "arn", "aws_sqs_queue")?,
                    matched.filtered,
                    Some(self.nodes[matched.node].address.clone()),
                )
            }
            "google_pubsub_subscription" => {
                let mut source =
                    self.endpoint_arg(at, each, block, "topic", "google_pubsub_topic")?;
                let target = &self.nodes[node];
                if target
                    .provisioned
                    .as_ref()
                    .map_or(target.pattern, |p| p.pattern)
                {
                    return None;
                }
                let target = Endpoint {
                    name: target
                        .provisioned
                        .as_ref()
                        .map_or(target.name.as_ref(), |p| p.name.as_ref())?
                        .clone(),
                    through: target
                        .provisioned
                        .as_ref()
                        .map_or(&target.overridable, |p| &p.through)
                        .iter()
                        .cloned()
                        .collect(),
                };
                let filtered = block.args.get("filter").is_some_and(|arg| {
                    let found = self.value(at, each, &arg.expr, 0);
                    source.through.extend(found.through);
                    found.val != Val::Known(Value::String(String::new()))
                });
                (source, target, filtered, None)
            }
            _ => return None,
        };
        let broker = match block.ty.as_str() {
            "aws_sns_topic_subscription" => "sns",
            "azurerm_servicebus_subscription" => "servicebus",
            "google_pubsub_subscription" => "pubsub",
            _ => "eventbridge",
        };
        let source_contract = format!("channel::{broker}::{}", source.name);
        let target_broker = if matches!(broker, "servicebus" | "pubsub") {
            broker
        } else {
            "sqs"
        };
        let target_contract = format!("channel::{target_broker}::{}", target.name);
        let mut reason = format!(
            "{target_contract} subscription {} (root {})",
            self.nodes[node].address, self.root
        );
        if let Some(rule) = via {
            reason += &format!(" via {rule}");
        }
        if filtered {
            reason += "; filter";
        }
        let through: BTreeSet<_> = source.through.union(&target.through).cloned().collect();
        for o in &through {
            reason += &format!(
                "; overridable: {}:{} {} ({})",
                o.path,
                o.line,
                o.variable,
                if o.tfvars { "tfvars" } else { "default" }
            );
        }
        Some(Subscription {
            source: source_contract,
            node: self.nodes[node].address.clone(),
            root: self.root.clone(),
            target: target_contract,
            inferred: filtered || !through.is_empty(),
            reason,
        })
    }
}
