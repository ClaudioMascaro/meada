//! AWS data and secret operations (#660), read only on constructed SDK clients.
use crate::contract::sdk::{self, Client, Operation, Service};
use crate::contract::{Found, Role};
use crate::resolve::Code;

const SERVICES: &[Service] = &[
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.s3.S3Client",
            go: "github.com/aws/aws-sdk-go-v2/service/s3",
            csharp: "Amazon.S3.AmazonS3Client",
        },
        kind: "s3",
        operations: &[
            Operation {
                method: "PutObject",
                field: "Bucket",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "GetObject",
                field: "Bucket",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "DeleteObject",
                field: "Bucket",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "HeadObject",
                field: "Bucket",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "ListObjects",
                field: "Bucket",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "ListObjectsV2",
                field: "Bucket",
                role: Role::Consumer,
                hierarchy: false,
            },
        ],
    },
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.dynamodb.DynamoDbClient",
            go: "github.com/aws/aws-sdk-go-v2/service/dynamodb",
            csharp: "Amazon.DynamoDBv2.AmazonDynamoDBClient",
        },
        kind: "dynamodb",
        operations: &[
            Operation {
                method: "PutItem",
                field: "TableName",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "UpdateItem",
                field: "TableName",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "DeleteItem",
                field: "TableName",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "BatchWriteItem",
                field: "RequestItems.*",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "GetItem",
                field: "TableName",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "Query",
                field: "TableName",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "Scan",
                field: "TableName",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "BatchGetItem",
                field: "RequestItems.*",
                role: Role::Consumer,
                hierarchy: false,
            },
        ],
    },
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.ssm.SsmClient",
            go: "github.com/aws/aws-sdk-go-v2/service/ssm",
            csharp: "Amazon.SimpleSystemsManagement.AmazonSimpleSystemsManagementClient",
        },
        kind: "ssm",
        operations: &[
            Operation {
                method: "GetParameter",
                field: "Name",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "GetParameters",
                field: "Names[]",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "GetParametersByPath",
                field: "Path",
                role: Role::Consumer,
                hierarchy: true,
            },
            Operation {
                method: "PutParameter",
                field: "Name",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "DeleteParameter",
                field: "Name",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "DeleteParameters",
                field: "Names[]",
                role: Role::Provider,
                hierarchy: false,
            },
        ],
    },
    Service {
        client: Client {
            java: "software.amazon.awssdk.services.secretsmanager.SecretsManagerClient",
            go: "github.com/aws/aws-sdk-go-v2/service/secretsmanager",
            csharp: "Amazon.SecretsManager.AmazonSecretsManagerClient",
        },
        kind: "secretsmanager",
        operations: &[
            Operation {
                method: "GetSecretValue",
                field: "SecretId",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "BatchGetSecretValue",
                field: "SecretIdList[]",
                role: Role::Consumer,
                hierarchy: false,
            },
            Operation {
                method: "PutSecretValue",
                field: "SecretId",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "UpdateSecret",
                field: "SecretId",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "CreateSecret",
                field: "Name",
                role: Role::Provider,
                hierarchy: false,
            },
            Operation {
                method: "DeleteSecret",
                field: "SecretId",
                role: Role::Provider,
                hierarchy: false,
            },
        ],
    },
];

pub(super) fn extract(code: &Code, indexed: &crate::index::Indexed) -> Vec<Found> {
    sdk::extract(code, "resource", SERVICES, Some(indexed))
}
