//! Official Azure Python sync/aio constructors, context-managed children and operations.
use super::{Argument, Code, Frame, Handle, Role, Stage, argument, imported};

pub(super) fn constructed<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let call = code.call(path, index)?;
    let spelling = imported(code, path, &call.scope, &call.callee)?;
    let (class, factory) = spelling
        .strip_suffix(".from_connection_string")
        .map_or((spelling.as_str(), false), |class| (class, true));
    let stage = match class {
        "azure.servicebus.ServiceBusClient" | "azure.servicebus.aio.ServiceBusClient" => Stage::Bus,
        "azure.eventhub.EventHubProducerClient" | "azure.eventhub.aio.EventHubProducerClient" => {
            Stage::EventProducer
        }
        "azure.eventhub.EventHubConsumerClient" | "azure.eventhub.aio.EventHubConsumerClient" => {
            Stage::EventConsumer
        }
        "azure.storage.blob.BlobServiceClient" | "azure.storage.blob.aio.BlobServiceClient" => {
            Stage::Storage
        }
        "azure.keyvault.secrets.SecretClient" | "azure.keyvault.secrets.aio.SecretClient"
            if !factory =>
        {
            Stage::Secret
        }
        _ => return None,
    };
    let name = if matches!(stage, Stage::EventProducer | Stage::EventConsumer) {
        argument(
            code,
            path,
            &call.scope,
            call.keyword("eventhub_name")
                .or_else(|| (!factory).then(|| call.args.get(1)).flatten()),
            frames,
            Some((path, index)),
        )
    } else {
        Argument::Unresolved
    };
    Some(Handle::named(stage, name))
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
        (Stage::Bus, "get_queue_sender") => Handle::named(Stage::Sender, name("queue_name", 0)),
        (Stage::Bus, "get_topic_sender") => Handle::named(Stage::Sender, name("topic_name", 0)),
        (Stage::Bus, "get_queue_receiver") => Handle::named(Stage::Receiver, name("queue_name", 0)),
        (Stage::Bus, "get_subscription_receiver") => {
            Handle::subscription(name("topic_name", 0), name("subscription_name", 1))
        }
        (Stage::Storage, "get_container_client") => {
            Handle::named(Stage::Container, name("container", 0))
        }
        (Stage::Storage, "get_blob_client") => Handle::named(Stage::Blob, name("container", 0)),
        (Stage::Container, "get_blob_client") => parent.stage(Stage::Blob),
        _ => return None,
    })
}
pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "send_messages"
            | "receive_messages"
            | "send_batch"
            | "receive"
            | "upload_blob"
            | "download_blob"
            | "get_secret"
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
        (Stage::Sender, "send_messages")
        | (Stage::EventProducer, "send_batch")
        | (Stage::Blob, "upload_blob") => Role::Provider,
        (Stage::Receiver | Stage::Subscription, "receive_messages")
        | (Stage::EventConsumer, "receive")
        | (Stage::Blob, "download_blob") => Role::Consumer,
        (Stage::Secret, "get_secret") => {
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
