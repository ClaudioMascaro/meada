package unbuilt

import (
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/dynamodb/types"
)

// The package's own `len` on another system: Meada reads one target's build (linux, amd64), where
// `len` is the builtin.
func len(items []types.TransactWriteItem) int {
	items[0].Put.TableName = aws.String("unbuilt-new")
	return 1
}
