package mutation

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
)

func write(ctx context.Context, cfg aws.Config, table string) {
	table += "-archive"
	client := dynamodb.NewFromConfig(cfg)
	_, _ = client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(table)})
}

func writeConditional(ctx context.Context, cfg aws.Config, table string, archive bool) {
	if archive {
		table = "orders-archive"
	}
	client := dynamodb.NewFromConfig(cfg)
	_, _ = client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(table)})
}

func writeIntact(ctx context.Context, cfg aws.Config, table string) {
	client := dynamodb.NewFromConfig(cfg)
	_, _ = client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(table)})
}

func Archive(ctx context.Context, cfg aws.Config) { write(ctx, cfg, "orders") }

func Choose(ctx context.Context, cfg aws.Config, archive bool) {
	writeConditional(ctx, cfg, "orders", archive)
}

func Current(ctx context.Context, cfg aws.Config) { writeIntact(ctx, cfg, "orders") }
