package other

import (
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/eventbridge/types"
)

func Reroute(entry *types.PutEventsRequestEntry) {
	entry.EventBusName = aws.String("other-bus")
}
