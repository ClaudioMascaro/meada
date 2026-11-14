package application

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

var cfg aws.Config

// One list two inline requests send: an SDK call reads it and changes nothing, so it keeps its
// table, unless it changes between the sends. Alone in its repository, since a write of a field
// the path reads (`x.Put = ..`) in any file unnames a list held in a name there.

func sentTwice(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("sent-twice")}}}
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func sentThenSet(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("set-old")}}}
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
	items[0] = types.TransactWriteItem{Put: &types.Put{TableName: aws.String("set-new")}}
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}
