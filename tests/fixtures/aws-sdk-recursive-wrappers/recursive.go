package application

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// A chain of wrappers enters a function once. A wrapper that calls itself with another value in
// the parameter the request reads sends what the first pass does not: each caller's site gets an
// unresolved side for it, beside the name its own pass gives (#886).

// A table name: the recursive call passes `b` where the request reads `a`.
func sendTable(ctx context.Context, n int, a, b string) {
	client := dynamodb.NewFromConfig(cfg)
	client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(a)})
	if n > 0 {
		sendTable(ctx, n-1, b, "")
	}
}

func placeTable(ctx context.Context) {
	sendTable(ctx, 1, "table-first", "table-second")
}

// A value made from the parameter the request reads.
func sendNext(ctx context.Context, n int, a string) {
	client := dynamodb.NewFromConfig(cfg)
	client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(a)})
	if n > 0 {
		sendNext(ctx, n-1, a+"-next")
	}
}

func placeNext(ctx context.Context) {
	sendNext(ctx, 1, "next-first")
}

// A list nobody passes: read where it stands, it is a write and, as a `ConditionCheck` is an
// entry too, a read of no table.
func aloneList(ctx context.Context, items []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

// A list of entries, the same way: what the next turn sends reads as `aloneList` does.
func sendList(ctx context.Context, n int, a, b []types.TransactWriteItem) {
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: a})
	if n > 0 {
		sendList(ctx, n-1, b, nil)
	}
}

func placeList(ctx context.Context) {
	x := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("list-first")}}}
	y := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("list-second")}}}
	sendList(ctx, 1, x, y)
}

// The second list changed by its caller: the first is still the caller's own.
func placeChanged(ctx context.Context) {
	x := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("changed-first")}}}
	y := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("changed-old")}}}
	y[0].Put.TableName = aws.String("changed-new")
	sendList(ctx, 1, x, y)
}

// Two wrappers that call each other, the second swapping the names.
func ping(ctx context.Context, n int, a, b string) {
	client := dynamodb.NewFromConfig(cfg)
	client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(a)})
	if n > 0 {
		pong(ctx, n-1, a, b)
	}
}

func pong(ctx context.Context, n int, a, b string) {
	ping(ctx, n, b, a)
}

func placePing(ctx context.Context) {
	ping(ctx, 1, "ping-first", "ping-second")
}

// Control: a retry passes its parameter on as it is, so another turn sends what the first did
// and the recursive call adds no side.
func retry(ctx context.Context, n int, table string) {
	client := dynamodb.NewFromConfig(cfg)
	client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(table)})
	if n > 0 {
		retry(ctx, n-1, table)
	}
}

func placeRetry(ctx context.Context) {
	retry(ctx, 3, "retried")
}

// Control: two wrappers that call each other and pass the name on as it is.
func tick(ctx context.Context, n int, table string) {
	client := dynamodb.NewFromConfig(cfg)
	client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(table)})
	if n > 0 {
		tock(ctx, n-1, table)
	}
}

func tock(ctx context.Context, n int, table string) {
	tick(ctx, n, table)
}

func placeTick(ctx context.Context) {
	tick(ctx, 1, "ticked")
}

// Nothing but its own cycle calls it: the wrapper is read where it stands.
func spin(ctx context.Context, n int, a, b string) {
	client := dynamodb.NewFromConfig(cfg)
	client.PutItem(ctx, &dynamodb.PutItemInput{TableName: aws.String(a)})
	if n > 0 {
		spin(ctx, n-1, b, a)
	}
}
