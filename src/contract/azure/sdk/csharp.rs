//! New Azure .NET forms. Legacy Service Bus sends/processors remain in channel/csharp.
use super::{Code, Frame, Handle, Role, Stage, Value, argument};

pub(super) fn constructed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    ty: &'a str,
    args: &'a [Value],
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let name = crate::names::known(code, path, scope, ty, |s| {
        matches!(
            s,
            "Azure.Messaging.ServiceBus.ServiceBusClient"
                | "Azure.Messaging.EventHubs.Producer.EventHubProducerClient"
                | "Azure.Messaging.EventHubs.Consumer.EventHubConsumerClient"
                | "Azure.Storage.Blobs.BlobServiceClient"
                | "Azure.Security.KeyVault.Secrets.SecretClient"
        )
    })?;
    let (stage, name) = match name.as_str() {
        "Azure.Messaging.ServiceBus.ServiceBusClient" => (Stage::Bus, None),
        "Azure.Messaging.EventHubs.Producer.EventHubProducerClient" => {
            (Stage::EventProducer, args.get(1))
        }
        "Azure.Messaging.EventHubs.Consumer.EventHubConsumerClient" => {
            (Stage::EventConsumer, args.get(2))
        }
        "Azure.Storage.Blobs.BlobServiceClient" => (Stage::Storage, None),
        _ => (Stage::Secret, None),
    };
    Some(Handle::named(
        stage,
        argument(code, path, scope, name, frames, None),
    ))
}
pub(super) fn step<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    parent: Handle,
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let call = code.call(path, index)?;
    let name = |keyword, n| {
        argument(
            code,
            path,
            &call.scope,
            call.keyword(keyword).or_else(|| call.args.get(n)),
            frames,
            Some((path, index)),
        )
    };
    Some(match (parent.stage, crate::resolve::method(&call.callee)) {
        (Stage::Bus, "CreateReceiver") => {
            let options = call.args.get(1).is_some_and(|value| {
                matches!(
                    code.value_within_at(
                        path,
                        &call.scope,
                        value,
                        frames,
                        Some((path, index)),
                        super::receiver_options,
                        crate::contract::sdk::mutations::writes,
                    ),
                    Value::New(ty, _) if super::receiver_options(&ty)
                )
            });
            if call.keyword("subscriptionName").is_some()
                || (call.args.len() >= 2 && !(call.args.len() == 2 && options))
            {
                Handle::subscription(name("topicName", 0), name("subscriptionName", 1))
            } else {
                Handle::named(Stage::Receiver, name("queueName", 0))
            }
        }
        (Stage::Storage, "GetBlobContainerClient") => {
            Handle::named(Stage::Container, name("blobContainerName", 0))
        }
        (Stage::Container, "GetBlobClient") => parent.stage(Stage::Blob),
        _ => return None,
    })
}
pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "ReceiveMessageAsync"
            | "ReceiveMessagesAsync"
            | "SendAsync"
            | "ReadEventsAsync"
            | "Upload"
            | "UploadAsync"
            | "DownloadTo"
            | "DownloadToAsync"
            | "UploadBlob"
            | "UploadBlobAsync"
            | "GetSecret"
            | "GetSecretAsync"
    )
}
pub(super) fn read<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    handle: Handle,
) -> Option<(Role, Handle)> {
    let call = code.call(path, index)?;
    let role = match (handle.stage, crate::resolve::method(&call.callee)) {
        (Stage::EventProducer, "SendAsync")
        | (Stage::Blob, "Upload" | "UploadAsync")
        | (Stage::Container, "UploadBlob" | "UploadBlobAsync") => Role::Provider,
        (Stage::Receiver | Stage::Subscription, "ReceiveMessageAsync" | "ReceiveMessagesAsync")
        | (Stage::EventConsumer, "ReadEventsAsync")
        | (Stage::Blob, "DownloadTo" | "DownloadToAsync") => Role::Consumer,
        (Stage::Secret, "GetSecret" | "GetSecretAsync") => {
            return Some((
                Role::Consumer,
                Handle::named(
                    Stage::Secret,
                    argument(
                        code,
                        path,
                        &call.scope,
                        call.keyword("name").or_else(|| call.args.first()),
                        &[],
                        Some((path, index)),
                    ),
                ),
            ));
        }
        _ => return None,
    };
    Some((role, handle))
}
