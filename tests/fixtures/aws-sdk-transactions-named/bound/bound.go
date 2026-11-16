package bound

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// A local `cap` hides the builtin: `cap(items)` here may change the list.
func capBound(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	cap := func(list []types.TransactWriteItem) int {
		list[0].Put.TableName = aws.String("bound-new")
		return 1
	}
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("bound-old")}}}
	in := &dynamodb.TransactWriteItemsInput{TransactItems: items}
	if cap(items) > 0 {
		client.TransactWriteItems(ctx, in)
	}
}
