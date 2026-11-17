package other

import (
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// Writes through a parameter, in another package: they reach a value a caller hands over,
// never one a function holds in a local it passes to nobody.

func Rename(q *types.Put) {
	q.TableName = aws.String("other")
}

func Replace(item *types.TransactWriteItem) {
	item.Put = &types.Put{TableName: aws.String("other-item")}
}

func Refill(in *dynamodb.TransactWriteItemsInput) {
	in.TransactItems = nil
}

func Reread(item *types.TransactGetItem) {
	item.Get = &types.Get{TableName: aws.String("other-get")}
}
