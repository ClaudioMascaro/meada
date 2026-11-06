//! The resource kind's AWS operation matrix and name arguments, shared by its language forms.
use crate::contract::{Found, Role, Site};
use crate::lang::{Value, property};
use crate::names::{self, Argument};

pub(super) struct Operation {
    pub service: &'static str,
    pub client: &'static str,
    pub ty: &'static str,
    pub js: &'static str,
    pub python: &'static str,
    key: &'static str,
    names: Names,
    role: Role,
}
enum Names {
    One,
    List,
    Keys,
    Writes,
    Reads,
    Path,
}
pub(super) const OPERATIONS: &[Operation] = &[
    Operation {
        service: "s3",
        client: "new S3Client()",
        ty: "aws_s3_bucket",
        js: "PutObjectCommand",
        python: "put_object",
        key: "Bucket",
        names: Names::One,
        role: Role::Provider,
    },
    Operation {
        service: "s3",
        client: "new S3Client()",
        ty: "aws_s3_bucket",
        js: "GetObjectCommand",
        python: "get_object",
        key: "Bucket",
        names: Names::One,
        role: Role::Consumer,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "PutItemCommand",
        python: "put_item",
        key: "TableName",
        names: Names::One,
        role: Role::Provider,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "UpdateItemCommand",
        python: "update_item",
        key: "TableName",
        names: Names::One,
        role: Role::Provider,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "DeleteItemCommand",
        python: "delete_item",
        key: "TableName",
        names: Names::One,
        role: Role::Provider,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "GetItemCommand",
        python: "get_item",
        key: "TableName",
        names: Names::One,
        role: Role::Consumer,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "QueryCommand",
        python: "query",
        key: "TableName",
        names: Names::One,
        role: Role::Consumer,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "ScanCommand",
        python: "scan",
        key: "TableName",
        names: Names::One,
        role: Role::Consumer,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "BatchWriteItemCommand",
        python: "batch_write_item",
        key: "RequestItems",
        names: Names::Keys,
        role: Role::Provider,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "BatchGetItemCommand",
        python: "batch_get_item",
        key: "RequestItems",
        names: Names::Keys,
        role: Role::Consumer,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "TransactWriteItemsCommand",
        python: "transact_write_items",
        key: "TransactItems",
        names: Names::Writes,
        role: Role::Provider,
    },
    Operation {
        service: "dynamodb",
        client: "new DynamoDBClient()",
        ty: "aws_dynamodb_table",
        js: "TransactGetItemsCommand",
        python: "transact_get_items",
        key: "TransactItems",
        names: Names::Reads,
        role: Role::Consumer,
    },
    Operation {
        service: "ssm",
        client: "new SSMClient()",
        ty: "aws_ssm_parameter",
        js: "GetParameterCommand",
        python: "get_parameter",
        key: "Name",
        names: Names::One,
        role: Role::Consumer,
    },
    Operation {
        service: "ssm",
        client: "new SSMClient()",
        ty: "aws_ssm_parameter",
        js: "GetParametersCommand",
        python: "get_parameters",
        key: "Names",
        names: Names::List,
        role: Role::Consumer,
    },
    Operation {
        service: "ssm",
        client: "new SSMClient()",
        ty: "aws_ssm_parameter",
        js: "GetParametersByPathCommand",
        python: "get_parameters_by_path",
        key: "Path",
        names: Names::Path,
        role: Role::Consumer,
    },
    Operation {
        service: "secretsmanager",
        client: "new SecretsManagerClient()",
        ty: "aws_secretsmanager_secret",
        js: "GetSecretValueCommand",
        python: "get_secret_value",
        key: "SecretId",
        names: Names::One,
        role: Role::Consumer,
    },
    Operation {
        service: "secretsmanager",
        client: "new SecretsManagerClient()",
        ty: "aws_secretsmanager_secret",
        js: "BatchGetSecretValueCommand",
        python: "batch_get_secret_value",
        key: "SecretIdList",
        names: Names::List,
        role: Role::Consumer,
    },
];

impl Operation {
    pub fn read(&self, properties: &[(String, Value)]) -> Option<Vec<(Argument, Role)>> {
        let value = property(properties, self.key)
            .ok()?
            .unwrap_or(&Value::Unknown);
        let names: Vec<Argument> = match (&self.names, value) {
            (Names::One, value) => vec![names::argument(value)?],
            (Names::Path, value) => vec![match names::argument(value)? {
                Argument::Literal(_) => Argument::Unresolved,
                other => other,
            }],
            (Names::List, Value::List(items)) => {
                items.iter().map(names::argument).collect::<Option<_>>()?
            }
            (Names::Keys, Value::Object(entries)) => entries
                .iter()
                .map(|(key, value)| match (key.as_str(), value) {
                    ("[]", Value::List(pair)) => {
                        names::argument(pair.first().unwrap_or(&Value::Unknown))
                    }
                    ("...", Value::Ref(_)) => None,
                    ("...", _) | ("[]", _) => Some(Argument::Unresolved),
                    (key, _) => Some(Argument::Literal(key.into())),
                })
                .collect::<Option<_>>()?,
            (Names::Writes | Names::Reads, Value::List(items)) => {
                let mut out = Vec::new();
                let keys: &[&str] = if matches!(self.names, Names::Reads) {
                    &["Get"]
                } else {
                    &["Put", "Update", "Delete", "ConditionCheck"]
                };
                for item in items {
                    let Value::Object(entries) = item else {
                        out.push((names::argument(item)?, self.role));
                        continue;
                    };
                    for key in keys {
                        if let Some(value) = property(entries, key).ok()? {
                            let name = match value {
                                Value::Object(fields) => names::argument(
                                    property(fields, "TableName")
                                        .ok()?
                                        .unwrap_or(&Value::Unknown),
                                )?,
                                _ => names::argument(value)?,
                            };
                            let role = if *key == "ConditionCheck" {
                                Role::Consumer
                            } else {
                                self.role
                            };
                            out.push((name, role));
                        }
                    }
                }
                return Some(out);
            }
            (_, Value::Ref(_)) => return None,
            _ => vec![Argument::Unresolved],
        };
        Some(names.into_iter().map(|name| (name, self.role)).collect())
    }

    pub fn found(&self, (argument, role): (Argument, Role), (path, index): (&str, usize)) -> Found {
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
        let contract = match name {
            Some(name) => format!("resource::{}::{name}", self.service),
            None => format!("resource::{}", self.service),
        };
        Found {
            role,
            site: Site::Call {
                path: path.into(),
                index,
            },
            contract: Some(contract),
            via: Some(via),
            method: None,
            fragments: Vec::new(),
            shape: None,
        }
    }
}
