//! Azure Go exact package factories and receiver/positional-name forms.
use super::{Code, Frame, Handle, Role, Stage, argument};

pub(super) fn constructed<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    frames: &[Frame<'a>],
) -> Option<Handle> {
    let call = code.call(path, index)?;
    let outside = code.outside(path, &call.scope, &call.callee, call.by_path)?;
    if !outside.direct {
        return None;
    }
    let (stage, name) = match outside.spelled.as_str() {
        "github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus.NewClient"
        | "github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus.NewClientFromConnectionString" => {
            (Stage::Bus, None)
        }
        "github.com/Azure/azure-sdk-for-go/sdk/messaging/azeventhubs.NewProducerClient"
        | "github.com/Azure/azure-sdk-for-go/sdk/messaging/azeventhubs.NewProducerClientFromConnectionString" => {
            (Stage::EventProducer, call.args.get(1))
        }
        "github.com/Azure/azure-sdk-for-go/sdk/messaging/azeventhubs.NewConsumerClient"
        | "github.com/Azure/azure-sdk-for-go/sdk/messaging/azeventhubs.NewConsumerClientFromConnectionString" => {
            (Stage::EventConsumer, call.args.get(1))
        }
        "github.com/Azure/azure-sdk-for-go/sdk/storage/azblob.NewClient"
        | "github.com/Azure/azure-sdk-for-go/sdk/storage/azblob.NewClientWithNoCredential"
        | "github.com/Azure/azure-sdk-for-go/sdk/storage/azblob.NewClientWithSharedKeyCredential"
        | "github.com/Azure/azure-sdk-for-go/sdk/storage/azblob.NewClientFromConnectionString" => {
            (Stage::Storage, None)
        }
        "github.com/Azure/azure-sdk-for-go/sdk/security/keyvault/azsecrets.NewClient" => {
            (Stage::Secret, None)
        }
        _ => return None,
    };
    Some(Handle::named(
        stage,
        argument(code, path, &call.scope, name, frames, Some((path, index))),
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
        (Stage::Bus, "NewSender") => Handle::named(Stage::Sender, name(0)),
        (Stage::Bus, "NewReceiverForQueue") => Handle::named(Stage::Receiver, name(0)),
        (Stage::Bus, "NewReceiverForSubscription") => Handle::subscription(name(0), name(1)),
        (Stage::EventConsumer, "NewPartitionClient") => parent.stage(Stage::EventPartition),
        _ => return None,
    })
}
pub(super) fn operation(method: &str) -> bool {
    matches!(
        method,
        "SendMessage"
            | "SendMessageBatch"
            | "ReceiveMessages"
            | "SendEventDataBatch"
            | "ReceiveEvents"
            | "UploadBuffer"
            | "UploadFile"
            | "UploadStream"
            | "DownloadBuffer"
            | "DownloadFile"
            | "DownloadStream"
            | "GetSecret"
    )
}
pub(super) fn read<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    handle: Handle,
) -> Option<(Role, Handle)> {
    let call = code.call(path, index)?;
    let method = crate::resolve::method(&call.callee);
    let role = match (handle.stage, method) {
        (Stage::Sender, "SendMessage" | "SendMessageBatch")
        | (Stage::EventProducer, "SendEventDataBatch") => Role::Provider,
        (Stage::Receiver | Stage::Subscription, "ReceiveMessages")
        | (Stage::EventPartition, "ReceiveEvents") => Role::Consumer,
        (
            Stage::Storage,
            "UploadBuffer" | "UploadFile" | "UploadStream" | "DownloadBuffer" | "DownloadFile"
            | "DownloadStream",
        ) => {
            let role = if method.starts_with("Upload") {
                Role::Provider
            } else {
                Role::Consumer
            };
            return Some((
                role,
                Handle::named(
                    Stage::Container,
                    argument(
                        code,
                        path,
                        &call.scope,
                        call.args.get(1),
                        &[],
                        Some((path, index)),
                    ),
                ),
            ));
        }
        (Stage::Secret, "GetSecret") => {
            return Some((
                Role::Consumer,
                Handle::named(
                    Stage::Secret,
                    argument(
                        code,
                        path,
                        &call.scope,
                        call.args.get(1),
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
