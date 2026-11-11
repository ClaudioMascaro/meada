package application

import (
	"context"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/sqs"
	"github.com/aws/aws-sdk-go-v2/service/sns"
	"github.com/aws/aws-sdk-go-v2/service/eventbridge"
	"github.com/aws/aws-sdk-go-v2/service/kinesis"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/ssm"
	"github.com/aws/aws-sdk-go-v2/service/secretsmanager"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
	eventbridgetypes "github.com/aws/aws-sdk-go-v2/service/eventbridge/types"
)

func sendMessage(ctx context.Context) {
	client := sqs.NewFromConfig(cfg)
	client.SendMessage(ctx, &sqs.SendMessageInput{QueueUrl: aws.String("https://sqs.us-east-1.amazonaws.com/123456789012/orders")})
}

func sendMessageBatch(ctx context.Context) {
	client := sqs.NewFromConfig(cfg)
	client.SendMessageBatch(ctx, &sqs.SendMessageBatchInput{QueueUrl: aws.String("https://sqs.us-east-1.amazonaws.com/123456789012/orders-batch")})
}

func receiveMessage(ctx context.Context) {
	client := sqs.NewFromConfig(cfg)
	client.ReceiveMessage(ctx, &sqs.ReceiveMessageInput{QueueUrl: aws.String("https://sqs.us-east-1.amazonaws.com/123456789012/orders-in")})
}

func publish(ctx context.Context) {
	client := sns.NewFromConfig(cfg)
	client.Publish(ctx, &sns.PublishInput{TopicArn: aws.String("arn:aws:sns:us-east-1:123456789012:alerts")})
}

func publishBatch(ctx context.Context) {
	client := sns.NewFromConfig(cfg)
	client.PublishBatch(ctx, &sns.PublishBatchInput{TopicArn: aws.String("arn:aws:sns:us-east-1:123456789012:alerts-batch")})
}

func putEvents(ctx context.Context) {
	client := eventbridge.NewFromConfig(cfg)
	client.PutEvents(ctx, &eventbridge.PutEventsInput{Entries: []eventbridgetypes.PutEventsRequestEntry{{EventBusName: aws.String("events")}, {Source: aws.String("app")}}})
}

func putRecord(ctx context.Context) {
	client := kinesis.NewFromConfig(cfg)
	client.PutRecord(ctx, &kinesis.PutRecordInput{StreamName: aws.String("records")})
}

func putRecords(ctx context.Context) {
	client := kinesis.NewFromConfig(cfg)
	client.PutRecords(ctx, &kinesis.PutRecordsInput{StreamName: aws.String("records-batch")})
}

func putObject(ctx context.Context) {
	client := s3.NewFromConfig(cfg)
	client.PutObject(ctx, &s3.PutObjectInput{Bucket: aws.String("orders-bucket")})
}

func getObject(ctx context.Context) {
	client := s3.NewFromConfig(cfg)
	client.GetObject(ctx, &s3.GetObjectInput{Bucket: aws.String("orders-bucket")})
}

func deleteObject(ctx context.Context) {
	client := s3.NewFromConfig(cfg)
	client.DeleteObject(ctx, &s3.DeleteObjectInput{Bucket: aws.String("orders-bucket")})
}

func headObject(ctx context.Context) {
	client := s3.NewFromConfig(cfg)
	client.HeadObject(ctx, &s3.HeadObjectInput{Bucket: aws.String("orders-bucket")})
}

func listObjects(ctx context.Context) {
	client := s3.NewFromConfig(cfg)
	client.ListObjects(ctx, &s3.ListObjectsInput{Bucket: aws.String("orders-bucket")})
}

func listObjectsV2(ctx context.Context) {
	client := s3.NewFromConfig(cfg)
	client.ListObjectsV2(ctx, &s3.ListObjectsV2Input{Bucket: aws.String("orders-bucket")})
}

func putItem(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String("orders")})
}

func updateItem(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.UpdateItem(ctx, &dynamodb.UpdateItemInput{TableName: aws.String("orders")})
}

func deleteItem(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.DeleteItem(ctx, &dynamodb.DeleteItemInput{TableName: aws.String("orders")})
}

func batchWriteItem(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.BatchWriteItem(ctx, &dynamodb.BatchWriteItemInput{RequestItems: map[string][]types.WriteRequest{"orders": {}}})
}

func getItem(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.GetItem(ctx, &dynamodb.GetItemInput{TableName: aws.String("orders")})
}

func query(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.Query(ctx, &dynamodb.QueryInput{TableName: aws.String("orders")})
}

func scan(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.Scan(ctx, &dynamodb.ScanInput{TableName: aws.String("orders")})
}

func batchGetItem(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.BatchGetItem(ctx, &dynamodb.BatchGetItemInput{RequestItems: map[string]types.KeysAndAttributes{"orders": {}}})
}

func getParameter(ctx context.Context) {
	client := ssm.NewFromConfig(cfg)
	client.GetParameter(ctx, &ssm.GetParameterInput{Name: aws.String("/orders/token")})
}

func getParameters(ctx context.Context) {
	client := ssm.NewFromConfig(cfg)
	client.GetParameters(ctx, &ssm.GetParametersInput{Names: []string{"/orders/a"}})
}

func getParametersByPath(ctx context.Context) {
	client := ssm.NewFromConfig(cfg)
	client.GetParametersByPath(ctx, &ssm.GetParametersByPathInput{Path: aws.String("/orders/")})
}

func putParameter(ctx context.Context) {
	client := ssm.NewFromConfig(cfg)
	client.PutParameter(ctx, &ssm.PutParameterInput{Name: aws.String("/orders/token")})
}

func deleteParameter(ctx context.Context) {
	client := ssm.NewFromConfig(cfg)
	client.DeleteParameter(ctx, &ssm.DeleteParameterInput{Name: aws.String("/orders/old")})
}

func deleteParameters(ctx context.Context) {
	client := ssm.NewFromConfig(cfg)
	client.DeleteParameters(ctx, &ssm.DeleteParametersInput{Names: []string{"/orders/b"}})
}

func getSecretValue(ctx context.Context) {
	client := secretsmanager.NewFromConfig(cfg)
	client.GetSecretValue(ctx, &secretsmanager.GetSecretValueInput{SecretId: aws.String("arn:aws:secretsmanager:us-east-1:123456789012:secret:orders/token-AbCd12")})
}

func batchGetSecretValue(ctx context.Context) {
	client := secretsmanager.NewFromConfig(cfg)
	client.BatchGetSecretValue(ctx, &secretsmanager.BatchGetSecretValueInput{SecretIdList: []string{"orders/a"}})
}

func putSecretValue(ctx context.Context) {
	client := secretsmanager.NewFromConfig(cfg)
	client.PutSecretValue(ctx, &secretsmanager.PutSecretValueInput{SecretId: aws.String("orders/token")})
}

func updateSecret(ctx context.Context) {
	client := secretsmanager.NewFromConfig(cfg)
	client.UpdateSecret(ctx, &secretsmanager.UpdateSecretInput{SecretId: aws.String("orders/token")})
}

func createSecret(ctx context.Context) {
	client := secretsmanager.NewFromConfig(cfg)
	client.CreateSecret(ctx, &secretsmanager.CreateSecretInput{Name: aws.String("orders/new")})
}

func deleteSecret(ctx context.Context) {
	client := secretsmanager.NewFromConfig(cfg)
	client.DeleteSecret(ctx, &secretsmanager.DeleteSecretInput{SecretId: aws.String("orders/old")})
}

func transactWriteItems(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{
		{Put: &types.Put{TableName: aws.String("orders")}},
		{ConditionCheck: &types.ConditionCheck{TableName: aws.String("customers")}},
	}})
}

func transactGetItems(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactGetItems(ctx, &dynamodb.TransactGetItemsInput{TransactItems: []types.TransactGetItem{
		{Get: &types.Get{TableName: aws.String("orders")}},
	}})
}
