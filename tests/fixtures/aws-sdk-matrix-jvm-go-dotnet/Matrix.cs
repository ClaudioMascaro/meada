using System.Collections.Generic;
using System.Threading.Tasks;
using Amazon.SQS;
using Amazon.SQS.Model;
using Amazon.SimpleNotificationService;
using Amazon.SimpleNotificationService.Model;
using Amazon.EventBridge;
using Amazon.EventBridge.Model;
using Amazon.Kinesis;
using Amazon.Kinesis.Model;
using Amazon.S3;
using Amazon.S3.Model;
using Amazon.DynamoDBv2;
using Amazon.DynamoDBv2.Model;
using Amazon.SimpleSystemsManagement;
using Amazon.SimpleSystemsManagement.Model;
using Amazon.SecretsManager;
using Amazon.SecretsManager.Model;

class Matrix {
    async Task SendMessage() {
        var client = new AmazonSQSClient();
        await client.SendMessageAsync(new SendMessageRequest { QueueUrl = "https://sqs.us-east-1.amazonaws.com/123456789012/orders" });
    }

    async Task SendMessageBatch() {
        var client = new AmazonSQSClient();
        await client.SendMessageBatchAsync(new SendMessageBatchRequest { QueueUrl = "https://sqs.us-east-1.amazonaws.com/123456789012/orders-batch" });
    }

    async Task ReceiveMessage() {
        var client = new AmazonSQSClient();
        await client.ReceiveMessageAsync(new ReceiveMessageRequest { QueueUrl = "https://sqs.us-east-1.amazonaws.com/123456789012/orders-in" });
    }

    async Task Publish() {
        var client = new AmazonSimpleNotificationServiceClient();
        await client.PublishAsync(new PublishRequest { TopicArn = "arn:aws:sns:us-east-1:123456789012:alerts" });
    }

    async Task PublishBatch() {
        var client = new AmazonSimpleNotificationServiceClient();
        await client.PublishBatchAsync(new PublishBatchRequest { TopicArn = "arn:aws:sns:us-east-1:123456789012:alerts-batch" });
    }

    async Task PutEvents() {
        var client = new AmazonEventBridgeClient();
        await client.PutEventsAsync(new PutEventsRequest { Entries = new List<PutEventsRequestEntry> { new PutEventsRequestEntry { EventBusName = "events" }, new PutEventsRequestEntry { Source = "app" } } });
    }

    async Task PutRecord() {
        var client = new AmazonKinesisClient();
        await client.PutRecordAsync(new PutRecordRequest { StreamName = "records" });
    }

    async Task PutRecords() {
        var client = new AmazonKinesisClient();
        await client.PutRecordsAsync(new PutRecordsRequest { StreamName = "records-batch" });
    }

    async Task PutObject() {
        var client = new AmazonS3Client();
        await client.PutObjectAsync(new PutObjectRequest { BucketName = "orders-bucket" });
    }

    async Task GetObject() {
        var client = new AmazonS3Client();
        await client.GetObjectAsync(new GetObjectRequest { BucketName = "orders-bucket" });
    }

    async Task DeleteObject() {
        var client = new AmazonS3Client();
        await client.DeleteObjectAsync(new DeleteObjectRequest { BucketName = "orders-bucket" });
    }

    async Task HeadObject() {
        var client = new AmazonS3Client();
        await client.GetObjectMetadataAsync(new GetObjectMetadataRequest { BucketName = "orders-bucket" });
    }

    async Task HeadObjectBlocking() {
        var client = new AmazonS3Client();
        client.GetObjectMetadata(new GetObjectMetadataRequest { BucketName = "orders-bucket" });
    }

    async Task ListObjects() {
        var client = new AmazonS3Client();
        await client.ListObjectsAsync(new ListObjectsRequest { BucketName = "orders-bucket" });
    }

    async Task ListObjectsV2() {
        var client = new AmazonS3Client();
        await client.ListObjectsV2Async(new ListObjectsV2Request { BucketName = "orders-bucket" });
    }

    async Task PutItem() {
        var client = new AmazonDynamoDBClient();
        await client.PutItemAsync(new PutItemRequest { TableName = "orders" });
    }

    async Task UpdateItem() {
        var client = new AmazonDynamoDBClient();
        await client.UpdateItemAsync(new UpdateItemRequest { TableName = "orders" });
    }

    async Task DeleteItem() {
        var client = new AmazonDynamoDBClient();
        await client.DeleteItemAsync(new DeleteItemRequest { TableName = "orders" });
    }

    async Task BatchWriteItem() {
        var client = new AmazonDynamoDBClient();
        await client.BatchWriteItemAsync(new BatchWriteItemRequest { RequestItems = new Dictionary<string, List<WriteRequest>> { ["orders"] = new List<WriteRequest>() } });
    }

    async Task GetItem() {
        var client = new AmazonDynamoDBClient();
        await client.GetItemAsync(new GetItemRequest { TableName = "orders" });
    }

    async Task Query() {
        var client = new AmazonDynamoDBClient();
        await client.QueryAsync(new QueryRequest { TableName = "orders" });
    }

    async Task Scan() {
        var client = new AmazonDynamoDBClient();
        await client.ScanAsync(new ScanRequest { TableName = "orders" });
    }

    async Task BatchGetItem() {
        var client = new AmazonDynamoDBClient();
        await client.BatchGetItemAsync(new BatchGetItemRequest { RequestItems = new Dictionary<string, KeysAndAttributes> { ["orders"] = new KeysAndAttributes() } });
    }

    async Task GetParameter() {
        var client = new AmazonSimpleSystemsManagementClient();
        await client.GetParameterAsync(new GetParameterRequest { Name = "/orders/token" });
    }

    async Task GetParameters() {
        var client = new AmazonSimpleSystemsManagementClient();
        await client.GetParametersAsync(new GetParametersRequest { Names = new List<string> { "/orders/a" } });
    }

    async Task GetParametersByPath() {
        var client = new AmazonSimpleSystemsManagementClient();
        await client.GetParametersByPathAsync(new GetParametersByPathRequest { Path = "/orders/" });
    }

    async Task PutParameter() {
        var client = new AmazonSimpleSystemsManagementClient();
        await client.PutParameterAsync(new PutParameterRequest { Name = "/orders/token" });
    }

    async Task DeleteParameter() {
        var client = new AmazonSimpleSystemsManagementClient();
        await client.DeleteParameterAsync(new DeleteParameterRequest { Name = "/orders/old" });
    }

    async Task DeleteParameters() {
        var client = new AmazonSimpleSystemsManagementClient();
        await client.DeleteParametersAsync(new DeleteParametersRequest { Names = new List<string> { "/orders/b" } });
    }

    async Task GetSecretValue() {
        var client = new AmazonSecretsManagerClient();
        await client.GetSecretValueAsync(new GetSecretValueRequest { SecretId = "arn:aws:secretsmanager:us-east-1:123456789012:secret:orders/token-AbCd12" });
    }

    async Task BatchGetSecretValue() {
        var client = new AmazonSecretsManagerClient();
        await client.BatchGetSecretValueAsync(new BatchGetSecretValueRequest { SecretIdList = new List<string> { "orders/a" } });
    }

    async Task PutSecretValue() {
        var client = new AmazonSecretsManagerClient();
        await client.PutSecretValueAsync(new PutSecretValueRequest { SecretId = "orders/token" });
    }

    async Task UpdateSecret() {
        var client = new AmazonSecretsManagerClient();
        await client.UpdateSecretAsync(new UpdateSecretRequest { SecretId = "orders/token" });
    }

    async Task CreateSecret() {
        var client = new AmazonSecretsManagerClient();
        await client.CreateSecretAsync(new CreateSecretRequest { Name = "orders/new" });
    }

    async Task DeleteSecret() {
        var client = new AmazonSecretsManagerClient();
        await client.DeleteSecretAsync(new DeleteSecretRequest { SecretId = "orders/old" });
    }

    async Task TransactWriteItems() {
        var client = new AmazonDynamoDBClient();
        await client.TransactWriteItemsAsync(new TransactWriteItemsRequest {
            TransactItems = new List<TransactWriteItem> {
                new TransactWriteItem { Put = new Put { TableName = "orders" } },
                new TransactWriteItem { ConditionCheck = new ConditionCheck { TableName = "customers" } }
            }
        });
    }

    async Task TransactGetItems() {
        var client = new AmazonDynamoDBClient();
        await client.TransactGetItemsAsync(new TransactGetItemsRequest {
            TransactItems = new List<TransactGetItem> { new TransactGetItem { Get = new Get { TableName = "orders" } } }
        });
    }
}
