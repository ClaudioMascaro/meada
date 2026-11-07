//! AWS SDK v3 resource operations, guarded by both SDK client and command origins.
use super::aws::OPERATIONS;
use crate::contract::Found;
use crate::resolve::Code;

pub(super) fn sides(code: &Code) -> Vec<Found> {
    if !code.pack().extensions().contains(&"js") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            if !call.callee.ends_with(".send") {
                continue;
            }
            let Some((client, package, command)) =
                crate::names::ts::constructor_argument(code, path, n)
            else {
                continue;
            };
            if package != client.package {
                continue;
            }
            let Some(operation) = OPERATIONS.iter().find(|op| {
                op.js == command
                    && op.client == client.expr
                    && client.package
                        == format!(
                            "@aws-sdk/client-{}",
                            if op.service == "secretsmanager" {
                                "secrets-manager"
                            } else {
                                op.service
                            }
                        )
            }) else {
                continue;
            };
            for found in crate::names::ts::constructor_options(code, path, n, &|fields| {
                operation.read(fields)
            }) {
                out.extend(
                    found
                        .found
                        .into_iter()
                        .map(|name| operation.found(name, found.site)),
                );
            }
        }
    }
    out
}
