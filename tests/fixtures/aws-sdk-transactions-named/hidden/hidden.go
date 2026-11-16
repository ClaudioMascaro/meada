package hidden

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// A package that declares its own `len` hides the builtin: `len(items)` here may change the list.
func len(items []types.TransactWriteItem) int {
	items[0].Put.TableName = aws.String("hidden-new")
	return 1
}

func hiddenLen(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("hidden-old")}}}
	in := &dynamodb.TransactWriteItemsInput{TransactItems: items}
	if len(items) > 0 {
		client.TransactWriteItems(ctx, in)
	}
}
