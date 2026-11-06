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
