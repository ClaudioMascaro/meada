package split

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

func splitLen(ctx context.Context) {
	client := dynamodb.NewFromConfig(cfg)
	items := []types.TransactWriteItem{{Put: &types.Put{TableName: aws.String("split-old")}}}
	in := &dynamodb.TransactWriteItemsInput{TransactItems: items}
	if len(items) > 0 {
		client.TransactWriteItems(ctx, in)
	}
}
