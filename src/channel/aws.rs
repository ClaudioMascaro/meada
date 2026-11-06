//! AWS messaging operations (#660), read only on constructed SDK clients.
use crate::contract::sdk::{self, Client, Operation, Service};
use crate::contract::{Found, Role};
use crate::resolve::Code;

const SERVICES: &[Service] = &[
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.sqs.SqsClient",
            go: "github.com/aws/aws-sdk-go-v2/service/sqs",
            csharp: "Amazon.SQS.AmazonSQSClient",
        },
        kind: "sqs",
        operations: &[
            Operation {
                method: "SendMessage",
                field: "QueueUrl",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "SendMessageBatch",
                field: "QueueUrl",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "ReceiveMessage",
                field: "QueueUrl",
                role: Role::Consumer,
                hierarchy: false,
            },
        ],
    },
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.sns.SnsClient",
            go: "github.com/aws/aws-sdk-go-v2/service/sns",
            csharp: "Amazon.SimpleNotificationService.AmazonSimpleNotificationServiceClient",
        },
        kind: "sns",
        operations: &[
            Operation {
                method: "Publish",
                field: "TopicArn",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "PublishBatch",
                field: "TopicArn",
                role: Role::Provider,
                hierarchy: false,
            },
        ],
    },
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.eventbridge.EventBridgeClient",
            go: "github.com/aws/aws-sdk-go-v2/service/eventbridge",
            csharp: "Amazon.EventBridge.AmazonEventBridgeClient",
        },
        kind: "eventbridge",
        operations: &[Operation {
            method: "PutEvents",
            field: "Entries[].EventBusName",
            role: Role::Provider,
            hierarchy: false,
        }],
    },
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.kinesis.KinesisClient",
            go: "github.com/aws/aws-sdk-go-v2/service/kinesis",
            csharp: "Amazon.Kinesis.AmazonKinesisClient",
        },
        kind: "kinesis",
        operations: &[
            Operation {
                method: "PutRecord",
                field: "StreamName",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "PutRecords",
                field: "StreamName",
                role: Role::Provider,
                hierarchy: false,
            },
        ],
    },
];

pub(super) fn extract(code: &Code, indexed: Option<&crate::index::Indexed>) -> Vec<Found> {
    sdk::extract(code, "channel", SERVICES, indexed)
}

/// Preserve the older language-reader tests at the same message seam while the shared
/// constructor-backed extractor now owns their AWS forms.
#[cfg(test)]
pub(super) fn test_messages(
    code: &Code,
    indexed: Option<&crate::index::Indexed>,
) -> Vec<super::message::Message> {
    use super::message::{Handler, Message, Side};
    extract(code, indexed)
        .into_iter()
        .filter_map(|f| {
            let rest = f.contract?.strip_prefix("channel::")?.to_string();
            let (broker, name) = rest
                .split_once("::")
                .map_or((rest.as_str(), None), |(b, n)| (b, Some(n.to_string())));
            let broker = match broker {
                "sqs" => "sqs",
                "sns" => "sns",
                "kinesis" => "kinesis",
                "eventbridge" => "eventbridge",
                _ => return None,
            };
            let (path, call, role) = match f.site {
                crate::contract::Site::Call { path, index } => (path, index, Side::Producer),
                crate::contract::Site::Def { path, name } => {
                    (path.clone(), 0, Side::Consumer(Handler::Def { path, name }))
                }
                _ => return None,
            };
            Some(Message {
                broker,
                name,
                path,
                call,
                role,
            })
        })
        .collect()
}
