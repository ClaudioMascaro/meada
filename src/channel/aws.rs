//! AWS source-channel operation matrix, shared by the channel kind's language readers.
use crate::contract::{Found, Role, Site};
use crate::lang::{Value, property};
use crate::names::{self, Argument};

pub(super) struct Operation {
    pub service: &'static str,
    pub client: &'static str,
    pub js: &'static str,
    pub python: &'static str,
    broker: &'static str,
    ty: &'static str,
}
pub(super) const OPERATIONS: &[Operation] = &[
    Operation {
        service: "sns",
        client: "new SNSClient()",
        js: "PublishCommand",
        python: "publish",
        broker: "sns",
        ty: "aws_sns_topic",
    },
    Operation {
        service: "sns",
        client: "new SNSClient()",
        js: "PublishBatchCommand",
        python: "publish_batch",
        broker: "sns",
        ty: "aws_sns_topic",
    },
    Operation {
        service: "events",
        client: "new EventBridgeClient()",
        js: "PutEventsCommand",
        python: "put_events",
        broker: "eventbridge",
        ty: "aws_cloudwatch_event_bus",
    },
    Operation {
        service: "kinesis",
        client: "new KinesisClient()",
        js: "PutRecordCommand",
        python: "put_record",
        broker: "kinesis",
        ty: "aws_kinesis_stream",
    },
    Operation {
        service: "kinesis",
        client: "new KinesisClient()",
        js: "PutRecordsCommand",
        python: "put_records",
        broker: "kinesis",
        ty: "aws_kinesis_stream",
    },
];
impl Operation {
    pub fn package(&self) -> String {
        format!("@aws-sdk/client-{}", self.broker)
    }
    pub fn read(&self, fields: &[(String, Value)]) -> Option<Vec<Argument>> {
        match self.broker {
            "sns" => {
                // Phone-number and endpoint publishes do not name a topic.
                let Some(value) = property(fields, "TopicArn").ok()? else {
                    return Some(Vec::new());
                };
                Some(vec![names::argument(value)?])
            }
            "kinesis" => {
                let value = property(fields, "StreamARN")
                    .ok()?
                    .or(property(fields, "StreamName").ok()?)
                    .unwrap_or(&Value::Unknown);
                Some(vec![names::argument(value)?])
            }
            "eventbridge" => match property(fields, "Entries").ok()? {
                Some(Value::List(items)) => items
                    .iter()
                    .map(|item| match item {
                        Value::Object(fields) => {
                            let bus = property(fields, "EventBusName").ok()?;
                            match bus {
                                Some(bus) => names::argument(bus),
                                None => Some(Argument::Literal("default".into())),
                            }
                        }
                        _ => names::argument(item),
                    })
                    .collect(),
                Some(Value::Ref(_)) => None,
                _ => Some(vec![Argument::Unresolved]),
            },
            _ => Some(Vec::new()),
        }
    }
    pub fn found(&self, argument: Argument, (path, index): (&str, usize)) -> Found {
        let (name, via) = match argument {
            Argument::Literal(name) => {
                let name = crate::iac::types::entry("terraform", self.ty)
                    .map(|entry| entry.read(&name))
                    .unwrap_or(&name);
                let name = (!name.is_empty() && !name.starts_with("arn:") && !name.contains("://"))
                    .then(|| name.to_string());
                (name, "sdk:aws".into())
            }
            Argument::Environment(name) => (None, format!("env:{name}")),
            Argument::Unresolved => (None, "sdk:aws".into()),
        };
        Found {
            identity: None,
            role: Role::Provider,
            site: Site::Call {
                path: path.into(),
                index,
            },
            contract: Some(super::contract(self.broker, name.as_deref())),
            via: Some(via),
            method: None,
            fragments: Vec::new(),
            shape: None,
        }
    }
}

use crate::contract::sdk::{self, Client, Operation as SdkOperation, Service};
use crate::resolve::Code;

const SERVICES: &[Service] = &[
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.sqs.SqsClient",
            go: "github.com/aws/aws-sdk-go-v2/service/sqs",
            csharp: "Amazon.SQS.AmazonSQSClient",
        },
        declared: &[
            "Amazon.SQS.IAmazonSQS",
            "Amazon.SQS.AmazonSQSClient",
            "software.amazon.awssdk.services.sqs.SqsClient",
            "software.amazon.awssdk.services.sqs.SqsAsyncClient",
            "github.com/aws/aws-sdk-go-v2/service/sqs.Client",
        ],
        kind: "sqs",
        operations: &[
            SdkOperation {
                method: "SendMessage",
                field: "QueueUrl",
                role: Role::Provider,
                hierarchy: false,
            },
            SdkOperation {
                method: "SendMessageBatch",
                field: "QueueUrl",
                role: Role::Provider,
                hierarchy: false,
            },
            SdkOperation {
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
        declared: &[],
        kind: "sns",
        operations: &[
            SdkOperation {
                method: "Publish",
                field: "TopicArn",
                role: Role::Provider,
                hierarchy: false,
            },
            SdkOperation {
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
        declared: &[],
        kind: "eventbridge",
        operations: &[SdkOperation {
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
        declared: &[],
        kind: "kinesis",
        operations: &[
            SdkOperation {
                method: "PutRecord",
                field: "StreamName",
                role: Role::Provider,
                hierarchy: false,
            },
            SdkOperation {
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
                environment: None,
                broker,
                name,
                path,
                call,
                role,
            })
        })
        .collect()
}
