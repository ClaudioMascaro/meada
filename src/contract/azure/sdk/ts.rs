//! Official Node Azure SDK constructor, child-client and operation inventory.
use super::{Code, Frame, Handle, Role, Stage, Value, argument, imported};

pub(super) fn constructed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    ty: &'a str,
    args: &'a [Value],
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let (stage, name) = match imported(code, path, scope, ty)?.as_str() {
        "@azure/service-bus.ServiceBusClient" => (Stage::Bus, None),
        "@azure/event-hubs.EventHubProducerClient" => (Stage::EventProducer, args.get(1)),
        "@azure/event-hubs.EventHubConsumerClient" => (Stage::EventConsumer, args.get(2)),
        "@azure/storage-blob.BlobServiceClient" => (Stage::Storage, None),
        "@azure/keyvault-secrets.SecretClient" => (Stage::Secret, None),
        _ => return None,
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
    let name = || {
        argument(
            code,
            path,
            &call.scope,
            call.args.first(),
            frames,
            Some((path, index)),
        )
    };
    Some(match (parent.stage, crate::resolve::method(&call.callee)) {
        (Stage::Bus, "createSender") => Handle::named(Stage::Sender, name()),
        (Stage::Bus, "createReceiver") => {
            if call
                .args
                .get(1)
                .is_some_and(|v| matches!(v, Value::Object(_)))
                || call.args.len() == 1
            {
                Handle::named(Stage::Receiver, name())
            } else {
                Handle::subscription(
                    name(),
                    argument(
                        code,
                        path,
                        &call.scope,
                        call.args.get(1),
                        frames,
                        Some((path, index)),
                    ),
                )
            }
        }
        (Stage::Storage, "getContainerClient") => Handle::named(Stage::Container, name()),
        (Stage::Container, "getBlobClient") => parent.stage(Stage::Blob),
        (Stage::Container, "getBlockBlobClient") => parent.stage(Stage::BlockBlob),
        _ => return None,
    })
}
pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "sendMessages"
            | "receiveMessages"
            | "subscribe"
            | "sendBatch"
            | "upload"
            | "uploadData"
            | "uploadFile"
            | "uploadStream"
            | "download"
            | "downloadToBuffer"
            | "downloadToFile"
            | "getSecret"
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
        (Stage::Sender, "sendMessages")
        | (Stage::EventProducer, "sendBatch")
        | (Stage::BlockBlob, "upload" | "uploadData" | "uploadFile" | "uploadStream") => {
            Role::Provider
        }
        (Stage::Receiver | Stage::Subscription, "receiveMessages" | "subscribe")
        | (Stage::EventConsumer, "subscribe")
        | (Stage::Blob | Stage::BlockBlob, "download" | "downloadToBuffer" | "downloadToFile") => {
            Role::Consumer
        }
        (Stage::Secret, "getSecret") => {
            return Some((
                Role::Consumer,
                Handle::named(
                    Stage::Secret,
                    argument(
                        code,
                        path,
                        &call.scope,
                        call.args.first(),
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
