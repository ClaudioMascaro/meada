package hidden

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// `len(items)` is this package's function (`len.go`), which may change the list: the wrapper's
// parameter is mentioned a third time, and its caller's list is not followed.
func sendHidden(ctx context.Context, items []types.TransactWriteItem) {
	if len(items) == 0 {
		return
	}
	client := dynamodb.NewFromConfig(cfg)
	client.TransactWriteItems(ctx, &dynamodb.TransactWriteItemsInput{TransactItems: items})
}

func hiddenCounted(ctx context.Context) {
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("hidden-old")}}}
	sendHidden(ctx, items)
}
