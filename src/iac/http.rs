//! Finite API Gateway route declarations, evaluated once with their exact resource targets.

mod rest;

use super::attribute::Attribute;
use super::*;

pub(super) fn reads(ty: &str) -> bool {
    matches!(
        ty,
        "aws_apigatewayv2_api"
            | "aws_apigatewayv2_route"
            | "aws_apigatewayv2_integration"
            | "aws_api_gateway_rest_api"
            | "aws_api_gateway_resource"
            | "aws_api_gateway_method"
            | "aws_api_gateway_integration"
    )
}

#[derive(Clone)]
struct Input<'m> {
    at: usize,
    each: Option<Each>,
    block: &'m Block,
}

impl<'m> Eval<'m> {
    fn http_overridable(&self, input: &Input<'m>) -> bool {
        input.each.as_ref().is_some_and(|e| !e.through.is_empty())
            || !self.instance_through(input.at).is_empty()
    }
    fn http_text(&mut self, input: &Input<'m>, key: &str) -> Option<Resolved> {
        let found = self.value(
            input.at,
            input.each.as_ref(),
            &input.block.args.get(key)?.expr,
            0,
        );
        let Val::Known(Value::String(text)) = found.val else {
            return None;
        };
        Some(Resolved {
            text,
            overridable: !found.through.is_empty(),
        })
    }

    fn http_attribute(
        &mut self,
        input: &Input<'m>,
        key: &str,
        ty: &str,
        name: &str,
    ) -> Option<Attribute> {
        let found = self.resource_attribute(
            input.at,
            input.each.as_ref(),
            &input.block.args.get(key)?.expr,
            0,
        )?;
        (self.nodes[found.node].ty == ty && found.name == name).then_some(found)
    }

    fn http_integration(&mut self, input: &Input<'m>) -> Option<Attribute> {
        let arg = &input.block.args.get("target")?.expr;
        let at = self.unalias(input.at, input.each.as_ref(), arg, 0)?;
        let Expression::StringTemplate(template) = at.expr else {
            return None;
        };
        let elements: Vec<_> = template.iter().collect();
        let [Element::Literal(prefix), Element::Interpolation(target)] = elements.as_slice() else {
            return None;
        };
        if prefix.value() != "integrations/" {
            return None;
        }
        let mut found = self.resource_attribute(at.at, at.each.as_ref(), &target.expr, 0)?;
        if self.nodes[found.node].ty != "aws_apigatewayv2_integration" || found.name != "id" {
            return None;
        }
        found.through.extend(at.through);
        Some(found)
    }

    /// A reference identifies one local function; a literal Lambda ARN proves only the repo.
    fn http_lambda(
        &mut self,
        input: &Input<'m>,
        key: &str,
        invocation: bool,
    ) -> Option<(Option<usize>, bool)> {
        let expr = &input.block.args.get(key)?.expr;
        if let Some(found) = self.resource_attribute(input.at, input.each.as_ref(), expr, 0)
            && self.nodes[found.node].ty == "aws_lambda_function"
            && (found.name == "invoke_arn" || (!invocation && found.name == "arn"))
        {
            return Some((Some(found.node), !found.through.is_empty()));
        }
        let found = self.value(input.at, input.each.as_ref(), expr, 0);
        if let Val::Known(Value::String(text)) = found.val {
            return (lambda_uri(&text) && (!invocation || text.ends_with("/invocations")))
                .then_some((None, !found.through.is_empty()));
        }
        let at = self.unalias(input.at, input.each.as_ref(), expr, 0)?;
        let Expression::StringTemplate(template) = at.expr else {
            return None;
        };
        let (mut prefix, mut suffix, mut target) = (String::new(), String::new(), None);
        let mut inferred = !at.through.is_empty();
        for element in template.iter() {
            let text = match element {
                Element::Literal(s) => s.value().clone(),
                Element::Interpolation(i) => {
                    if let Some(found) =
                        self.resource_attribute(at.at, at.each.as_ref(), &i.expr, 0)
                        && self.nodes[found.node].ty == "aws_lambda_function"
                        && found.name == "arn"
                    {
                        if target.replace(found.node).is_some() {
                            return None;
                        }
                        inferred |= !found.through.is_empty();
                        continue;
                    }
                    let found = self.value(at.at, at.each.as_ref(), &i.expr, 0);
                    let Val::Known(Value::String(text)) = found.val else {
                        return None;
                    };
                    inferred |= !found.through.is_empty();
                    text
                }
                Element::Directive(_) => return None,
            };
            if target.is_some() {
                suffix.push_str(&text);
            } else {
                prefix.push_str(&text);
            }
            if prefix.len() + suffix.len() > EVAL_VALUE_MAX {
                return None;
            }
        }
        let target = target?;
        ((!invocation && prefix.is_empty() && suffix.is_empty())
            || (invocation_prefix(&prefix) && suffix == "/invocations"))
            .then_some((Some(target), inferred))
    }

    fn http_put(
        &mut self,
        node: usize,
        source: &Input<'m>,
        key: &str,
        route: Resolved,
        inferred: bool,
    ) {
        let inferred = inferred || self.http_overridable(source);
        let mut fields = route.text.split_whitespace();
        let (Some(method), Some(path), None) = (fields.next(), fields.next(), fields.next()) else {
            return;
        };
        let route = Route {
            method: Resolved {
                text: method.to_uppercase(),
                overridable: route.overridable || inferred,
            },
            path: Resolved {
                text: path.into(),
                overridable: route.overridable || inferred,
            },
            at: format!("{}#{}.{}", source.block.path, source.block.key_path(), key),
        };
        if self.charge(route.size()) {
            self.nodes[node].routes.push(route);
        }
    }

    pub(super) fn http_routes(&mut self, pending: &[(usize, usize, &'m Block, Option<Each>)]) {
        let inputs: HashMap<_, _> = pending
            .iter()
            .filter(|(node, _, block, _)| {
                block.kind == Kind::Resource
                    && reads(&block.ty)
                    && self
                        .node_inputs
                        .get(&self.nodes[*node].address)
                        .is_some_and(|(at, _)| at == node)
            })
            .map(|(node, at, block, each)| {
                (
                    *node,
                    Input {
                        at: *at,
                        each: each.clone(),
                        block,
                    },
                )
            })
            .collect();
        // Iterate the declaration order, not a map's randomized order, when charging budgets.
        for &(node, _, block, _) in pending {
            let Some(input) = inputs.get(&node) else {
                continue;
            };
            if block.ty == "aws_apigatewayv2_api" {
                let Some(protocol) = self
                    .http_text(input, "protocol_type")
                    .filter(|p| p.text == "HTTP")
                else {
                    continue;
                };
                let (Some(route), Some((target, inferred))) = (
                    self.http_text(input, "route_key"),
                    self.http_lambda(input, "target", false),
                ) else {
                    continue;
                };
                self.http_put(
                    target.unwrap_or(node),
                    input,
                    "route_key",
                    route,
                    inferred || protocol.overridable,
                );
            }
            if block.ty != "aws_apigatewayv2_route" {
                continue;
            }
            let (Some(api), Some(integration), Some(route)) = (
                self.http_attribute(input, "api_id", "aws_apigatewayv2_api", "id"),
                self.http_integration(input),
                self.http_text(input, "route_key"),
            ) else {
                continue;
            };
            let (Some(api_input), Some(integration_input)) =
                (inputs.get(&api.node), inputs.get(&integration.node))
            else {
                continue;
            };
            let Some(protocol) = self
                .http_text(api_input, "protocol_type")
                .filter(|p| p.text == "HTTP")
            else {
                continue;
            };
            let Some(kind) = self
                .http_text(integration_input, "integration_type")
                .filter(|p| p.text == "AWS_PROXY")
            else {
                continue;
            };
            if integration_input
                .block
                .args
                .contains_key("integration_subtype")
            {
                continue;
            }
            let Some(integration_api) =
                self.http_attribute(integration_input, "api_id", "aws_apigatewayv2_api", "id")
            else {
                continue;
            };
            if integration_api.node != api.node {
                continue;
            }
            let Some((target, inferred)) =
                self.http_lambda(integration_input, "integration_uri", false)
            else {
                continue;
            };
            let inferred = inferred
                || protocol.overridable
                || kind.overridable
                || !api.through.is_empty()
                || !integration.through.is_empty()
                || !integration_api.through.is_empty();
            self.http_put(
                target.unwrap_or(api.node),
                input,
                "route_key",
                route,
                inferred,
            );
        }
        self.rest_routes(pending, &inputs);
    }
}

fn invocation_prefix(text: &str) -> bool {
    text.strip_prefix("arn:")
        .and_then(|s| s.split_once(":apigateway:"))
        .is_some_and(|(partition, rest)| {
            !partition.is_empty()
                && rest
                    .strip_suffix(":lambda:path/2015-03-31/functions/")
                    .is_some_and(|region| !region.is_empty() && !region.contains(':'))
        })
}

fn lambda_uri(text: &str) -> bool {
    let lambda = |arn: &str| {
        let fields: Vec<_> = arn.split(':').collect();
        fields.len() >= 7
            && fields.len() <= 8
            && fields[0] == "arn"
            && !fields[1].is_empty()
            && fields[2] == "lambda"
            && !fields[3].is_empty()
            && fields[4].len() == 12
            && fields[4].bytes().all(|b| b.is_ascii_digit())
            && fields[5] == "function"
            && !fields[6].is_empty()
            && fields.iter().all(|p| !p.contains(['*', '$', '/', ' ']))
    };
    if lambda(text) {
        return true;
    }
    let Some((prefix, function)) = text.split_once("/functions/") else {
        return false;
    };
    invocation_prefix(&format!("{prefix}/functions/"))
        && function.strip_suffix("/invocations").is_some_and(lambda)
}
