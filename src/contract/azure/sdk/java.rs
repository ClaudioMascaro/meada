//! Azure Java builder states and the documented synchronous operation inventory.
use super::{Code, Frame, Handle, Role, Stage, argument};

pub(super) fn constructed<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    ty: &'a str,
) -> Option<Handle> {
    let name = crate::names::known(code, path, scope, ty, |s| {
        matches!(
            s,
            "com.azure.messaging.servicebus.ServiceBusClientBuilder"
                | "com.azure.messaging.eventhubs.EventHubClientBuilder"
                | "com.azure.storage.blob.BlobServiceClientBuilder"
                | "com.azure.security.keyvault.secrets.SecretClientBuilder"
        )
    })?;
    Some(Handle::new(match name.as_str() {
        "com.azure.messaging.servicebus.ServiceBusClientBuilder" => Stage::BusBuilder,
        "com.azure.messaging.eventhubs.EventHubClientBuilder" => Stage::EventBuilder,
        "com.azure.storage.blob.BlobServiceClientBuilder" => Stage::StorageBuilder,
        _ => Stage::SecretBuilder,
    }))
}
pub(super) fn step<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    parent: Handle,
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let call = code.call(path, index)?;
    // A builder held in a local that receives another call changes in place.
    let builder = matches!(
        parent.stage,
        Stage::BusBuilder
            | Stage::SenderBuilder
            | Stage::ReceiverBuilder
            | Stage::ReceiverQueueBuilder
            | Stage::ReceiverTopicBuilder
            | Stage::EventBuilder
            | Stage::StorageBuilder
            | Stage::SecretBuilder
    );
    let parent = match &call.receiver {
        Some(crate::lang::Value::Ref(b))
            if builder
                && crate::contract::sdk::java::rebuilt(code, path, &call.scope, b, &call.range) =>
        {
            Handle::new(parent.stage)
        }
        _ => parent,
    };
    let name = |n| {
        argument(
            code,
            path,
            &call.scope,
            call.args.get(n),
            frames,
            Some((path, index)),
        )
    };
    Some(match (parent.stage, crate::resolve::method(&call.callee)) {
        (Stage::BusBuilder, "connectionString" | "credential")
        | (Stage::StorageBuilder, "endpoint" | "connectionString" | "credential")
        | (Stage::SecretBuilder, "vaultUrl" | "credential") => parent,
        (Stage::BusBuilder, "sender") => parent.stage(Stage::SenderBuilder),
        (Stage::BusBuilder, "receiver") => parent.stage(Stage::ReceiverBuilder),
        (Stage::SenderBuilder, "queueName" | "topicName") => {
            Handle::named(Stage::SenderBuilder, name(0))
        }
        (Stage::ReceiverBuilder | Stage::ReceiverQueueBuilder, "queueName")
            if parent.subscription.is_none() =>
        {
            Handle::named(Stage::ReceiverQueueBuilder, name(0))
        }
        (Stage::ReceiverBuilder | Stage::ReceiverTopicBuilder, "topicName") => Handle {
            stage: Stage::ReceiverTopicBuilder,
            name: name(0),
            subscription: parent.subscription,
        },
        (Stage::ReceiverBuilder | Stage::ReceiverTopicBuilder, "subscriptionName") => Handle {
            subscription: Some(name(0)),
            ..parent
        },
        (Stage::SenderBuilder, "buildClient") => parent.stage(Stage::Sender),
        (Stage::ReceiverQueueBuilder, "buildClient") => parent.stage(Stage::Receiver),
        (Stage::ReceiverTopicBuilder, "buildClient") if parent.subscription.is_some() => {
            parent.stage(Stage::Subscription)
        }
        (Stage::ReceiverBuilder, "buildClient") => parent.stage(Stage::Receiver),
        (Stage::EventBuilder, "connectionString" | "credential") => {
            Handle::named(Stage::EventBuilder, name(1))
        }
        (Stage::EventBuilder, "consumerGroup") => parent,
        (Stage::EventBuilder, "buildProducerClient") => parent.stage(Stage::EventProducer),
        (Stage::EventBuilder, "buildConsumerClient") => parent.stage(Stage::EventConsumer),
        (Stage::StorageBuilder, "buildClient") => parent.stage(Stage::Storage),
        (Stage::Storage, "getBlobContainerClient") => Handle::named(Stage::Container, name(0)),
        (Stage::Container, "getBlobClient") => parent.stage(Stage::Blob),
        (Stage::SecretBuilder, "buildClient") => parent.stage(Stage::Secret),
        _ => return None,
    })
}
pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "sendMessage"
            | "sendMessages"
            | "receiveMessages"
            | "send"
            | "receiveFromPartition"
            | "upload"
            | "uploadFromFile"
            | "downloadContent"
            | "downloadStream"
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
        (Stage::Sender, "sendMessage" | "sendMessages")
        | (Stage::EventProducer, "send")
        | (Stage::Blob, "upload" | "uploadFromFile") => Role::Provider,
        (Stage::Receiver | Stage::Subscription, "receiveMessages")
        | (Stage::EventConsumer, "receiveFromPartition")
        | (Stage::Blob, "downloadContent" | "downloadStream" | "downloadToFile") => Role::Consumer,
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
