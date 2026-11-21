package transactions

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

var fixtureConfig aws.Config

func closureWrite(ctx context.Context) {
	client := dynamodb.NewFromConfig(fixtureConfig)
	entry := &types.Put{TableName: aws.String("closure-before")}
	func() { entry.TableName = aws.String("closure-after") }()
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: entry}}})
}

func closureControl(ctx context.Context) {
	client := dynamodb.NewFromConfig(fixtureConfig)
	entry := &types.Put{TableName: aws.String("closure-control")}
	another := &types.Put{}
	func() { another.TableName = aws.String("unrelated-closure") }()
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: entry}}})
}

func goroutineWrite(ctx context.Context) {
	client := dynamodb.NewFromConfig(fixtureConfig)
	entry := &types.Put{TableName: aws.String("goroutine-before")}
	done := make(chan struct{})
	go func() {
		entry.TableName = aws.String("goroutine-after")
		close(done)
	}()
	<-done
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: entry}}})
}

func goroutineControl(ctx context.Context) {
	client := dynamodb.NewFromConfig(fixtureConfig)
	entry := &types.Put{TableName: aws.String("goroutine-control")}
	another := &types.Put{}
	go func() { another.TableName = aws.String("unrelated-goroutine") }()
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: entry}}})
}

func deferredWrite(ctx context.Context) {
	client := dynamodb.NewFromConfig(fixtureConfig)
	entry := &types.Put{TableName: aws.String("defer-before")}
	defer func() { entry.TableName = aws.String("defer-after") }()
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: entry}}})
}

func deferredControl(ctx context.Context) {
	client := dynamodb.NewFromConfig(fixtureConfig)
	entry := &types.Put{TableName: aws.String("defer-control")}
	another := &types.Put{}
	defer func() { another.TableName = aws.String("unrelated-defer") }()
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: []types.TransactWriteItem{{Put: entry}}})
}
