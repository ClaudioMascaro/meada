package split

import (
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// The package's own `len`, declared in another file than the call: it may change the list.
func len(items []types.TransactWriteItem) int {
	items[0].Put.TableName = aws.String("split-new")
	return 1
}
