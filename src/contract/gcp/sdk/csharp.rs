//! The documented .NET GCP factories and sync/async string or typed-name operations.
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
            "Google.Cloud.PubSub.V1.PublisherClient.CreateAsync"
                | "Google.Cloud.PubSub.V1.SubscriberClient.CreateAsync"
                | "Google.Cloud.Storage.V1.StorageClient.Create"
                | "Google.Cloud.SecretManager.V1.SecretManagerServiceClient.Create"
        )
    })?;
    let (stage, keyword) = match name.as_str() {
        "Google.Cloud.PubSub.V1.PublisherClient.CreateAsync" => (Stage::Topic, "topicName"),
        "Google.Cloud.PubSub.V1.SubscriberClient.CreateAsync" => {
            (Stage::Subscription, "subscriptionName")
        }
        "Google.Cloud.Storage.V1.StorageClient.Create" => (Stage::Storage, ""),
        _ => (Stage::Secret, ""),
    };
    Some(Handle::named(
        stage,
        argument(
            code,
            path,
            &call.scope,
            call.keyword(keyword).or_else(|| call.args.first()),
            frames,
        ),
    ))
}

pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "PublishAsync"
            | "StartAsync"
            | "UploadObject"
            | "UploadObjectAsync"
            | "DownloadObject"
            | "DownloadObjectAsync"
            | "AccessSecretVersion"
            | "AccessSecretVersionAsync"
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
        (Stage::Topic, "PublishAsync") => (Role::Provider, Stage::Topic, handle.name),
        (Stage::Subscription, "StartAsync") => (Role::Consumer, Stage::Subscription, handle.name),
        (Stage::Storage, "UploadObject" | "UploadObjectAsync") => (
            Role::Provider,
            Stage::Bucket,
            argument(
                code,
                path,
                &call.scope,
                call.keyword("bucket").or_else(|| call.args.first()),
                &[],
            ),
        ),
        (Stage::Storage, "DownloadObject" | "DownloadObjectAsync") => (
            Role::Consumer,
            Stage::Bucket,
            argument(
                code,
                path,
                &call.scope,
                call.keyword("bucket").or_else(|| call.args.first()),
                &[],
            ),
        ),
        (Stage::Secret, "AccessSecretVersion" | "AccessSecretVersionAsync") => (
            Role::Consumer,
            Stage::Secret,
            argument(
                code,
                path,
                &call.scope,
                call.keyword("name").or_else(|| call.args.first()),
                &[],
            ),
        ),
        _ => return None,
    })
}
