package hidden

import (
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// The package's own `len`, which the wrapper in the other file calls: it changes the list.
func len(items []types.TransactWriteItem) int {
	items[0].Put.TableName = aws.String("hidden-new")
	return 1
}
