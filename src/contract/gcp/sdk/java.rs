//! The documented GCP Java builders and operation arguments.
use super::{Argument, Code, Frame, Handle, Role, Stage, argument};

pub(super) fn constructed<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let call = code.call(path, index)?;
    if code.value_bound(path, &call.scope, call.callee.split('.').next()?) {
        return None;
    }
    let name = crate::names::known(code, path, &call.scope, &call.callee, |s| {
        matches!(
            s,
            "com.google.cloud.pubsub.v1.Publisher.newBuilder"
                | "com.google.cloud.pubsub.v1.Subscriber.newBuilder"
                | "com.google.cloud.storage.StorageOptions.newBuilder"
                | "com.google.cloud.secretmanager.v1.SecretManagerServiceClient.create"
        )
    })?;
    let stage = match name.as_str() {
        "com.google.cloud.pubsub.v1.Publisher.newBuilder" => Stage::TopicBuilder,
        "com.google.cloud.pubsub.v1.Subscriber.newBuilder" => Stage::SubscriptionBuilder,
        "com.google.cloud.storage.StorageOptions.newBuilder" => Stage::StorageBuilder,
        _ => Stage::Secret,
    };
    Some(Handle::named(
        stage,
        argument(code, path, &call.scope, call.args.first(), frames),
    ))
}

pub(super) fn step<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    parent: Handle,
) -> Option<Handle> {
    let call = code.call(path, index)?;
    let stage = match (parent.stage, crate::resolve::method(&call.callee)) {
        (Stage::TopicBuilder, "build") => Stage::Topic,
        (Stage::SubscriptionBuilder, "build") => Stage::Subscription,
        (Stage::StorageBuilder, "build") => Stage::StorageOptions,
        (Stage::StorageOptions, "getService") => Stage::Storage,
        _ => return None,
    };
    Some(Handle::named(stage, parent.name))
}

pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "publish" | "startAsync" | "create" | "readAllBytes" | "accessSecretVersion"
    )
}

pub(super) fn read<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    handle: Handle,
) -> Option<(Role, Stage, Argument)> {
    let call = code.call(path, index)?;
    Some(match (handle.stage, crate::resolve::method(&call.callee)) {
        (Stage::Topic, "publish") => (Role::Provider, Stage::Topic, handle.name),
        (Stage::Subscription, "startAsync") => (Role::Consumer, Stage::Subscription, handle.name),
        (Stage::Storage, "create") => (
            Role::Provider,
            Stage::Bucket,
            argument(code, path, &call.scope, call.args.first(), &[]),
        ),
        (Stage::Storage, "readAllBytes") => (
            Role::Consumer,
            Stage::Bucket,
            argument(code, path, &call.scope, call.args.first(), &[]),
        ),
        (Stage::Secret, "accessSecretVersion") => (
            Role::Consumer,
            Stage::Secret,
            argument(code, path, &call.scope, call.args.first(), &[]),
        ),
        _ => return None,
    })
}
