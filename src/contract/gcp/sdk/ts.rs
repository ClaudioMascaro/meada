//! The documented Node.js GCP client forms, shared by JavaScript and TypeScript syntax.
use super::{Argument, Code, Frame, Handle, Role, Stage, argument, field, imported};

pub(super) fn constructed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    ty: &'a str,
) -> Option<Handle> {
    let stage = match imported(code, path, scope, ty)?.as_str() {
        "@google-cloud/pubsub.PubSub" => Stage::Bus,
        "@google-cloud/storage.Storage" => Stage::Storage,
        "@google-cloud/secret-manager.SecretManagerServiceClient" => Stage::Secret,
        _ => return None,
    };
    Some(Handle::new(stage))
}

pub(super) fn step<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    parent: Handle,
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let call = code.call(path, index)?;
    let stage = match (parent.stage, crate::resolve::method(&call.callee)) {
        (Stage::Bus, "topic") => Stage::Topic,
        (Stage::Bus, "subscription") => Stage::Subscription,
        (Stage::Storage, "bucket") => Stage::Bucket,
        (Stage::Bucket, "file") => return Some(Handle::named(Stage::Object, parent.name)),
        _ => return None,
    };
    Some(Handle::named(
        stage,
        argument(code, path, &call.scope, call.args.first(), frames),
    ))
}

pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "publishMessage" | "on" | "save" | "download" | "accessSecretVersion"
    )
}

pub(super) fn read<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    handle: Handle,
) -> Option<(Role, Stage, Argument)> {
    let call = code.call(path, index)?;
    let role = match (handle.stage, crate::resolve::method(&call.callee)) {
        (Stage::Topic, "publishMessage") | (Stage::Object, "save") => Role::Provider,
        (Stage::Object, "download") => Role::Consumer,
        (Stage::Subscription, "on") if matches!(argument(code, path, &call.scope, call.args.first(), &[]), Argument::Literal(ref s) if s == "message") => {
            Role::Consumer
        }
        (Stage::Secret, "accessSecretVersion") => {
            let value = call
                .args
                .first()
                .map(|v| code.value_within(path, &call.scope, v, &[], super::typed_name));
            return Some((
                Role::Consumer,
                Stage::Secret,
                value
                    .as_ref()
                    .map(|v| field(code, path, v, "name"))
                    .unwrap_or(Argument::Unresolved),
            ));
        }
        _ => return None,
    };
    Some((role, handle.stage, handle.name))
}
