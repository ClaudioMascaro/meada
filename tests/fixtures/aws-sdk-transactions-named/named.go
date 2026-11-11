package application

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// A request, a list, an item or an entry held in a name changes in place. Its tables are exact
// only when the function writes the name twice: where it is declared and where the request reads
// it. Any other mention is a change the reader cannot follow.

func plainRequest(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("plain-request")}}}}
	client.TransactWriteItems(ctx, in)
}

func plainParts(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	p := &types.Put{TableName: aws.String("plain-parts")}
	item := types.TransactWriteItem{Put: p}
	items := []types.TransactWriteItem{item}
	in := &dynamodb.TransactWriteItemsInput{TransactItems: items}
	client.TransactWriteItems(ctx, in)
}

func plainGet(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactGetItemsInput{TransactItems: []types.TransactGetItem{{Get: &types.Get{TableName: aws.String("plain-get")}}}}
	client.TransactGetItems(ctx, in)
}

func throughRequest(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("through-old")}}}}
	in.TransactItems[0].Put.TableName = aws.String("through-new")
	client.TransactWriteItems(ctx, in)
}

func elementReplaced(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("element-old")}}}}
	in.TransactItems[0] = types.TransactWriteItem{Put: &types.Put{TableName: aws.String("element-new")}}
	client.TransactWriteItems(ctx, in)
}

func entryReplaced(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("entry-old")}}}}
	in.TransactItems[0].Put = &types.Put{TableName: aws.String("entry-new")}
	client.TransactWriteItems(ctx, in)
}

func sliceWritten(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("slice-old")}}}
	items[0].Put.TableName = aws.String("slice-new")
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func sliceReplaced(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("replaced-old")}}}
	items[0] = types.TransactWriteItem{Put: &types.Put{TableName: aws.String("replaced-new")}}
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func sliceHanded(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("handed-old")}}}
	swap(items)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func swap(items []types.TransactWriteItem) {
	items[0] = types.TransactWriteItem{Put: &types.Put{TableName: aws.String("handed-new")}}
}

func heldEntry(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	p := &types.Put{TableName: aws.String("held-old")}
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: p}}}
	p.TableName = aws.String("held-new")
	client.TransactWriteItems(ctx, in)
}

func itemWritten(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	item := types.TransactWriteItem{Put: &types.Put{TableName: aws.String("item-old")}}
	item.Put.TableName = aws.String("item-new")
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{item}})
}

func itemCopied(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	item := types.TransactWriteItem{Put: &types.Put{TableName: aws.String("copied-old")}}
	items := []types.TransactWriteItem{item}
	item.Put.TableName = aws.String("copied-new")
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func loopWritten(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("loop-old")}}}}
	for i := 0; i < 2; i++ {
		client.TransactWriteItems(ctx, in)
		in.TransactItems[0].Put.TableName = aws.String("loop-new")
	}
}

func getReplaced(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	in := &dynamodb.TransactGetItemsInput{TransactItems: []types.TransactGetItem{{Get: &types.Get{TableName: aws.String("get-old")}}}}
	in.TransactItems[0] = types.TransactGetItem{Get: &types.Get{TableName: aws.String("get-new")}}
	client.TransactGetItems(ctx, in)
}
