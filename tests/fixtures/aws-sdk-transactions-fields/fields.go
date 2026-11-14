package application

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// A field written through one name reaches no value that another function's local holds alone:
// on a transaction's path each name is judged by its own mentions (#774). The writes through
// other names are in `rename`, `putWritten`, the twins and the package `other`.

func heldEntry(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	p := &types.Put{TableName: aws.String("held-entry")}
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: p}}})
}

func heldList(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("held-list")}}}
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func heldRequest(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("held-request")}}}}
	client.TransactWriteItems(ctx, in)
}

func heldGet(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	g := &types.Get{TableName: aws.String("held-get")}
	client.TransactGetItems(ctx, &dynamodb.TransactGetItemsInput{TransactItems: []types.TransactGetItem{{Get: g}}})
}

// The twins: the same values written in place, a third mention of their names.

func writtenEntry(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	p := &types.Put{TableName: aws.String("written-entry-old")}
	p.TableName = aws.String("written-entry-new")
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: p}}})
}

func writtenList(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("written-list-old")}}}
	items[0].Put = &types.Put{TableName: aws.String("written-list-new")}
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func writtenRequest(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("written-request-old")}}}}
	in.TransactItems = []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("written-request-new")}}}
	client.TransactWriteItems(ctx, in)
}

func writtenGet(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	g := &types.Get{TableName: aws.String("written-get-old")}
	g.TableName = aws.String("written-get-new")
	client.TransactGetItems(ctx, &dynamodb.TransactGetItemsInput{TransactItems: []types.TransactGetItem{{Get: g}}})
}

// A request that is no list of entries keeps the rule it had: a `TableName` written anywhere
// leaves a named one unresolved.

func plainPut(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.PutItemInput{TableName: aws.String("plain-put")}
	client.PutItem(ctx, in)
}

func putWritten(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.PutItemInput{TableName: aws.String("put-old")}
	in.TableName = aws.String("put-new")
	client.PutItem(ctx, in)
}

// A struct of the repository's own with the SDK's field names.
type record struct {
	TableName     *string
	Put           *types.Put
	Get           *types.Get
	TransactItems []types.TransactWriteItem
}

func rename(r *record) {
	r.TableName = aws.String("record")
	r.Put = nil
	r.Get = nil
	r.TransactItems = nil
}
