//! REST API paths join only through the declared API/resource/public-method identity.

use super::*;

impl<'m> Eval<'m> {
    /// These method attributes repeat declarations, never provider-returned contents.
    fn declared_attribute(
        &mut self,
        input: &Input<'m>,
        key: &str,
        inputs: &HashMap<usize, Input<'m>>,
        depth: usize,
    ) -> Option<Attribute> {
        if depth > EVAL_DEPTH_MAX {
            return None;
        }
        let mut found = self.resource_attribute(
            input.at,
            input.each.as_ref(),
            &input.block.args.get(key)?.expr,
            depth,
        )?;
        if self.nodes[found.node].ty == "aws_api_gateway_method"
            && matches!(found.name.as_str(), "rest_api_id" | "resource_id")
        {
            let mut target =
                self.declared_attribute(inputs.get(&found.node)?, &found.name, inputs, depth + 1)?;
            target.through.append(&mut found.through);
            return Some(target);
        }
        Some(found)
    }

    fn public_method(
        &mut self,
        input: &Input<'m>,
        inputs: &HashMap<usize, Input<'m>>,
        depth: usize,
    ) -> Option<Resolved> {
        if depth > EVAL_DEPTH_MAX {
            return None;
        }
        if let Some(method) = self.http_text(input, "http_method") {
            return Some(method);
        }
        let found = self.resource_attribute(
            input.at,
            input.each.as_ref(),
            &input.block.args.get("http_method")?.expr,
            depth,
        )?;
        if self.nodes[found.node].ty != "aws_api_gateway_method" || found.name != "http_method" {
            return None;
        }
        let mut method = self.public_method(inputs.get(&found.node)?, inputs, depth + 1)?;
        method.overridable |= !found.through.is_empty();
        Some(method)
    }

    fn rest_identity(
        &mut self,
        input: &Input<'m>,
        inputs: &HashMap<usize, Input<'m>>,
    ) -> Option<(usize, Option<usize>, bool)> {
        let api = self.declared_attribute(input, "rest_api_id", inputs, 0)?;
        if self.nodes[api.node].ty != "aws_api_gateway_rest_api" || api.name != "id" {
            return None;
        }
        let resource = self.declared_attribute(input, "resource_id", inputs, 0)?;
        let target = match (
            self.nodes[resource.node].ty.as_str(),
            resource.name.as_str(),
        ) {
            ("aws_api_gateway_rest_api", "root_resource_id") if resource.node == api.node => None,
            ("aws_api_gateway_resource", "id") => Some(resource.node),
            _ => return None,
        };
        Some((
            api.node,
            target,
            !api.through.is_empty() || !resource.through.is_empty(),
        ))
    }

    fn rest_path(
        &mut self,
        api: usize,
        mut resource: Option<usize>,
        inputs: &HashMap<usize, Input<'m>>,
    ) -> Option<Resolved> {
        let mut parts = Vec::new();
        let (mut bytes, mut inferred) = (0usize, false);
        let mut seen = HashSet::new();
        while let Some(node) = resource {
            if !seen.insert(node) || seen.len() > EVAL_DEPTH_MAX {
                return None;
            }
            let input = inputs.get(&node)?;
            let owner =
                self.http_attribute(input, "rest_api_id", "aws_api_gateway_rest_api", "id")?;
            if owner.node != api {
                return None;
            }
            let part = self.http_text(input, "path_part")?;
            if part.text.is_empty() || part.text.contains('/') {
                return None;
            }
            bytes = bytes.checked_add(part.text.len() + 1)?;
            if bytes > EVAL_VALUE_MAX {
                return None;
            }
            inferred |= part.overridable || !owner.through.is_empty();
            parts.push(part.text);
            let parent = self.declared_attribute(input, "parent_id", inputs, 0)?;
            inferred |= !parent.through.is_empty();
            resource = match (self.nodes[parent.node].ty.as_str(), parent.name.as_str()) {
                ("aws_api_gateway_rest_api", "root_resource_id") if parent.node == api => None,
                ("aws_api_gateway_resource", "id") => Some(parent.node),
                _ => return None,
            };
        }
        parts.reverse();
        Some(Resolved {
            text: format!("/{}", parts.join("/")),
            overridable: inferred,
        })
    }

    pub(super) fn rest_routes(
        &mut self,
        pending: &[(usize, usize, &'m Block, Option<Each>)],
        inputs: &HashMap<usize, Input<'m>>,
    ) {
        let mut methods = HashMap::new();
        for &(node, _, block, _) in pending {
            if block.ty != "aws_api_gateway_method" {
                continue;
            }
            let Some(input) = inputs.get(&node) else {
                continue;
            };
            let (Some((api, resource, inferred)), Some(method)) = (
                self.rest_identity(input, inputs),
                self.public_method(input, inputs, 0),
            ) else {
                continue;
            };
            let Some(path) = self.rest_path(api, resource, inputs) else {
                continue;
            };
            let key = (api, resource, method.text.to_uppercase());
            let route = Resolved {
                text: format!("{} {}", key.2, path.text),
                overridable: inferred
                    || method.overridable
                    || path.overridable
                    || self.http_overridable(input),
            };
            methods
                .entry(key)
                .and_modify(|prior: &mut Resolved| prior.overridable &= route.overridable)
                .or_insert(route);
        }
        for &(node, _, block, _) in pending {
            if block.ty != "aws_api_gateway_integration" {
                continue;
            }
            let Some(input) = inputs.get(&node) else {
                continue;
            };
            let Some(kind) = self
                .http_text(input, "type")
                .filter(|p| matches!(p.text.as_str(), "AWS" | "AWS_PROXY"))
            else {
                continue;
            };
            let (Some((api, resource, inferred)), Some(method)) = (
                self.rest_identity(input, inputs),
                self.public_method(input, inputs, 0),
            ) else {
                continue;
            };
            let Some(route) = methods.get(&(api, resource, method.text.to_uppercase())) else {
                continue;
            };
            let Some((target, target_inferred)) = self.http_lambda(input, "uri", true) else {
                continue;
            };
            self.http_put(
                target.unwrap_or(api),
                input,
                "http_method",
                route.clone(),
                inferred || target_inferred || method.overridable || kind.overridable,
            );
        }
    }
}
