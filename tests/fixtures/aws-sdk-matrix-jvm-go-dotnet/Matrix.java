import java.util.List;
import java.util.Map;
import software.amazon.awssdk.services.sqs.model.SendMessageRequest;
import software.amazon.awssdk.services.sqs.SqsClient;
import software.amazon.awssdk.services.sqs.model.SendMessageBatchRequest;
import software.amazon.awssdk.services.sqs.model.ReceiveMessageRequest;
import software.amazon.awssdk.services.sns.model.PublishRequest;
import software.amazon.awssdk.services.sns.SnsClient;
import software.amazon.awssdk.services.sns.model.PublishBatchRequest;
import software.amazon.awssdk.services.eventbridge.model.PutEventsRequest;
import software.amazon.awssdk.services.eventbridge.model.PutEventsRequestEntry;
import software.amazon.awssdk.services.eventbridge.EventBridgeClient;
import software.amazon.awssdk.services.kinesis.model.PutRecordRequest;
import software.amazon.awssdk.services.kinesis.KinesisClient;
import software.amazon.awssdk.services.kinesis.model.PutRecordsRequest;
import software.amazon.awssdk.services.s3.model.PutObjectRequest;
import software.amazon.awssdk.services.s3.S3Client;
import software.amazon.awssdk.services.s3.model.GetObjectRequest;
import software.amazon.awssdk.services.s3.model.DeleteObjectRequest;
import software.amazon.awssdk.services.s3.model.HeadObjectRequest;
import software.amazon.awssdk.services.s3.model.ListObjectsRequest;
import software.amazon.awssdk.services.s3.model.ListObjectsV2Request;
import software.amazon.awssdk.services.dynamodb.model.PutItemRequest;
import software.amazon.awssdk.services.dynamodb.DynamoDbClient;
import software.amazon.awssdk.services.dynamodb.model.UpdateItemRequest;
import software.amazon.awssdk.services.dynamodb.model.DeleteItemRequest;
import software.amazon.awssdk.services.dynamodb.model.BatchWriteItemRequest;
import software.amazon.awssdk.services.dynamodb.model.WriteRequest;
import software.amazon.awssdk.services.dynamodb.model.GetItemRequest;
import software.amazon.awssdk.services.dynamodb.model.QueryRequest;
import software.amazon.awssdk.services.dynamodb.model.ScanRequest;
import software.amazon.awssdk.services.dynamodb.model.BatchGetItemRequest;
import software.amazon.awssdk.services.dynamodb.model.KeysAndAttributes;
import software.amazon.awssdk.services.ssm.model.GetParameterRequest;
import software.amazon.awssdk.services.ssm.SsmClient;
import software.amazon.awssdk.services.ssm.model.GetParametersRequest;
import software.amazon.awssdk.services.ssm.model.GetParametersByPathRequest;
import software.amazon.awssdk.services.ssm.model.PutParameterRequest;
import software.amazon.awssdk.services.ssm.model.DeleteParameterRequest;
import software.amazon.awssdk.services.ssm.model.DeleteParametersRequest;
import software.amazon.awssdk.services.secretsmanager.model.GetSecretValueRequest;
import software.amazon.awssdk.services.secretsmanager.SecretsManagerClient;
import software.amazon.awssdk.services.secretsmanager.model.BatchGetSecretValueRequest;
import software.amazon.awssdk.services.secretsmanager.model.PutSecretValueRequest;
import software.amazon.awssdk.services.secretsmanager.model.UpdateSecretRequest;
import software.amazon.awssdk.services.secretsmanager.model.CreateSecretRequest;
import software.amazon.awssdk.services.secretsmanager.model.DeleteSecretRequest;

class Matrix {
    void sendMessage() {
        var client = SqsClient.create();
        client.sendMessage(SendMessageRequest.builder().queueUrl("https://sqs.us-east-1.amazonaws.com/123456789012/orders").build());
    }

    void sendMessageBatch() {
        var client = SqsClient.create();
        client.sendMessageBatch(SendMessageBatchRequest.builder().queueUrl("https://sqs.us-east-1.amazonaws.com/123456789012/orders-batch").build());
    }

    void receiveMessage() {
        var client = SqsClient.create();
        client.receiveMessage(ReceiveMessageRequest.builder().queueUrl("https://sqs.us-east-1.amazonaws.com/123456789012/orders-in").build());
    }

    void publish() {
        var client = SnsClient.create();
        client.publish(PublishRequest.builder().topicArn("arn:aws:sns:us-east-1:123456789012:alerts").build());
    }

    void publishBatch() {
        var client = SnsClient.create();
        client.publishBatch(PublishBatchRequest.builder().topicArn("arn:aws:sns:us-east-1:123456789012:alerts-batch").build());
    }

    void putEvents() {
        var client = EventBridgeClient.create();
        client.putEvents(PutEventsRequest.builder().entries(PutEventsRequestEntry.builder().eventBusName("events").build(), PutEventsRequestEntry.builder().source("app").build()).build());
    }

    void putRecord() {
        var client = KinesisClient.create();
        client.putRecord(PutRecordRequest.builder().streamName("records").build());
    }

    void putRecords() {
        var client = KinesisClient.create();
        client.putRecords(PutRecordsRequest.builder().streamName("records-batch").build());
    }

    void putObject() {
        var client = S3Client.create();
        client.putObject(PutObjectRequest.builder().bucket("orders-bucket").build());
    }

    void getObject() {
        var client = S3Client.create();
        client.getObject(GetObjectRequest.builder().bucket("orders-bucket").build());
    }

    void deleteObject() {
        var client = S3Client.create();
        client.deleteObject(DeleteObjectRequest.builder().bucket("orders-bucket").build());
    }

    void headObject() {
        var client = S3Client.create();
        client.headObject(HeadObjectRequest.builder().bucket("orders-bucket").build());
    }

    void listObjects() {
        var client = S3Client.create();
        client.listObjects(ListObjectsRequest.builder().bucket("orders-bucket").build());
    }

    void listObjectsV2() {
        var client = S3Client.create();
        client.listObjectsV2(ListObjectsV2Request.builder().bucket("orders-bucket").build());
    }

    void putItem() {
        var client = DynamoDbClient.create();
        client.putItem(PutItemRequest.builder().tableName("orders").build());
    }

    void updateItem() {
        var client = DynamoDbClient.create();
        client.updateItem(UpdateItemRequest.builder().tableName("orders").build());
    }

    void deleteItem() {
        var client = DynamoDbClient.create();
        client.deleteItem(DeleteItemRequest.builder().tableName("orders").build());
    }

    void batchWriteItem() {
        var client = DynamoDbClient.create();
        client.batchWriteItem(BatchWriteItemRequest.builder().requestItems(Map.of("orders", List.of())).build());
    }

    void getItem() {
        var client = DynamoDbClient.create();
        client.getItem(GetItemRequest.builder().tableName("orders").build());
    }

    void query() {
        var client = DynamoDbClient.create();
        client.query(QueryRequest.builder().tableName("orders").build());
    }

    void scan() {
        var client = DynamoDbClient.create();
        client.scan(ScanRequest.builder().tableName("orders").build());
    }

    void batchGetItem() {
        var client = DynamoDbClient.create();
        client.batchGetItem(BatchGetItemRequest.builder().requestItems(Map.of("orders", KeysAndAttributes.builder().build())).build());
    }

    void getParameter() {
        var client = SsmClient.create();
        client.getParameter(GetParameterRequest.builder().name("/orders/token").build());
    }

    void getParameters() {
        var client = SsmClient.create();
        client.getParameters(GetParametersRequest.builder().names(List.of("/orders/a")).build());
    }

    void getParametersByPath() {
        var client = SsmClient.create();
        client.getParametersByPath(GetParametersByPathRequest.builder().path("/orders/").build());
    }

    void putParameter() {
        var client = SsmClient.create();
        client.putParameter(PutParameterRequest.builder().name("/orders/token").build());
    }

    void deleteParameter() {
        var client = SsmClient.create();
        client.deleteParameter(DeleteParameterRequest.builder().name("/orders/old").build());
    }

    void deleteParameters() {
        var client = SsmClient.create();
        client.deleteParameters(DeleteParametersRequest.builder().names(List.of("/orders/b")).build());
    }

    void getSecretValue() {
        var client = SecretsManagerClient.create();
        client.getSecretValue(GetSecretValueRequest.builder().secretId("arn:aws:secretsmanager:us-east-1:123456789012:secret:orders/token-AbCd12").build());
    }

    void batchGetSecretValue() {
        var client = SecretsManagerClient.create();
        client.batchGetSecretValue(BatchGetSecretValueRequest.builder().secretIdList(List.of("orders/a")).build());
    }

    void putSecretValue() {
        var client = SecretsManagerClient.create();
        client.putSecretValue(PutSecretValueRequest.builder().secretId("orders/token").build());
    }

    void updateSecret() {
        var client = SecretsManagerClient.create();
        client.updateSecret(UpdateSecretRequest.builder().secretId("orders/token").build());
    }

    void createSecret() {
        var client = SecretsManagerClient.create();
        client.createSecret(CreateSecretRequest.builder().name("orders/new").build());
    }

    void deleteSecret() {
        var client = SecretsManagerClient.create();
        client.deleteSecret(DeleteSecretRequest.builder().secretId("orders/old").build());
    }
}
