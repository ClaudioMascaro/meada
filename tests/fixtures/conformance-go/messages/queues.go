package messages

import (
	"context"
	"os"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/sqs"
)

const queueHost = "https://sqs.eu-west-1.amazonaws.com/123456789012/"

func SqsProduce(ctx context.Context, cfg aws.Config) {
	client := sqs.NewFromConfig(cfg)
	client.SendMessage(ctx, &sqs.SendMessageInput{
		QueueUrl:    aws.String("https://sqs.eu-west-1.amazonaws.com/123456789012/tasks"),
		MessageBody: aws.String("t"),
	})
	client.SendMessageBatch(ctx, &sqs.SendMessageBatchInput{QueueUrl: aws.String(queueHost + "invoices")})
	client.SendMessage(ctx, &sqs.SendMessageInput{QueueUrl: aws.String(os.Getenv("QUEUE_URL"))})
}

func SqsConsume(ctx context.Context, client *sqs.Client) {
	input := &sqs.ReceiveMessageInput{QueueUrl: aws.String(queueHost + "tasks")}
	client.ReceiveMessage(ctx, input)
	client.ReceiveMessage(ctx, &sqs.ReceiveMessageInput{QueueUrl: aws.String(queueHost + "reports")})
}
