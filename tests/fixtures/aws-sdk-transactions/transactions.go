package application

import (
	"context"
	"os"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

func write(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{
		{Put: &types.Put{TableName: aws.String("orders")}},
		{Update: &types.Update{TableName: aws.String("totals")}},
		{Delete: &types.Delete{TableName: aws.String("archive")}},
		{ConditionCheck: &types.ConditionCheck{TableName: aws.String("customers")}},
	}})
}

func read(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactGetItems(ctx, &dynamodb.TransactGetItemsInput{TransactItems: []types.TransactGetItem{
		{Get: &types.Get{TableName: aws.String("orders")}},
		{Get: &types.Get{TableName: aws.String("customers")}},
	}})
}

func fromEnvironment(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{
		{Put: &types.Put{TableName: aws.String(os.Getenv("TABLE"))}},
		{ConditionCheck: &types.ConditionCheck{TableName: aws.String(os.Getenv("CHECKED"))}},
	}})
}

func computed(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{
		{Put: &types.Put{TableName: aws.String(table())}},
		{ConditionCheck: &types.ConditionCheck{TableName: aws.String(table())}},
	}})
}
