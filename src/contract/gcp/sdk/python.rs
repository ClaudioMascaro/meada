//! The documented google-cloud Python client forms.
use super::{Argument, Code, Frame, Handle, Role, Stage, argument, field, imported};

pub(super) fn constructed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    callee: &'a str,
) -> Option<Handle> {
    let stage = match imported(code, path, scope, callee)?.as_str() {
        "google.cloud.pubsub_v1.PublisherClient" => Stage::Publisher,
        "google.cloud.pubsub_v1.SubscriberClient" => Stage::Subscriber,
        "google.cloud.storage.Client" => Stage::Storage,
        "google.cloud.secretmanager.SecretManagerServiceClient" => Stage::Secret,
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
    match (parent.stage, crate::resolve::method(&call.callee)) {
        (Stage::Storage, "bucket") => Some(Handle::named(
            Stage::Bucket,
            argument(
                code,
                path,
                &call.scope,
                call.args.first().or_else(|| call.keyword("bucket_name")),
                frames,
                Some((path, index)),
            ),
        )),
        (Stage::Bucket, "blob") => Some(Handle::named(Stage::Object, parent.name)),
        _ => None,
    }
}

pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "publish"
            | "subscribe"
            | "upload_from_string"
            | "download_as_bytes"
            | "access_secret_version"
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
        (Stage::Publisher, "publish") => (
            Role::Provider,
            Stage::Topic,
            argument(
                code,
                path,
                &call.scope,
                call.args.first().or_else(|| call.keyword("topic")),
                &[],
                Some((path, index)),
            ),
        ),
        (Stage::Subscriber, "subscribe") => (
            Role::Consumer,
            Stage::Subscription,
            argument(
                code,
                path,
                &call.scope,
                call.args.first().or_else(|| call.keyword("subscription")),
                &[],
                Some((path, index)),
            ),
        ),
        (Stage::Object, "upload_from_string") => (Role::Provider, Stage::Object, handle.name),
        (Stage::Object, "download_as_bytes") => (Role::Consumer, Stage::Object, handle.name),
        (Stage::Secret, "access_secret_version") => {
            let value = call
                .keyword("request")
                .or_else(|| call.args.first())
                .map(|v| {
                    code.value_within_at(
                        path,
                        &call.scope,
                        v,
                        &[],
                        Some((path, index)),
                        super::typed_name,
                        crate::contract::sdk::mutations::writes,
                    )
                });
            (
                Role::Consumer,
                Stage::Secret,
                value
                    .as_ref()
                    .map(|v| field(code, path, v, "name"))
                    .unwrap_or(Argument::Unresolved),
            )
        }
        _ => return None,
    })
}
