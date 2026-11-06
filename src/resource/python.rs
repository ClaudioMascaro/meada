//! boto3 resource operations on clients traced to boto3's constructors.
use super::aws::OPERATIONS;
use crate::contract::Found;
use crate::lang::Value;
use crate::resolve::{Code, Expand, Keywords};

pub(super) fn sides(code: &Code) -> Vec<Found> {
    if !code.pack().extensions().contains(&"py") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let Some((_, method)) = call.callee.rsplit_once('.') else {
                continue;
            };
            if !OPERATIONS.iter().any(|op| op.python == method) {
                continue;
            }
            let Some(first) = crate::names::python::factory_argument(
                code,
                path,
                call,
                &[
                    "boto3.client",
                    "boto3.Session.client",
                    "boto3.session.Session.client",
                ],
                &["client"],
                "service_name",
            ) else {
                continue;
            };
            let Some(operation) = OPERATIONS
                .iter()
                .find(|op| op.service == first && op.python == method)
            else {
                continue;
            };
            let read = |_: &[Value], kwargs: &Keywords| operation.read(kwargs);
            for found in code.expand(Expand::Call(path, n), &read) {
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
