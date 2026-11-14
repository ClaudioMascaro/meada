package application

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/eventbridge"
	"github.com/aws/aws-sdk-go-v2/service/eventbridge/types"
)

// `PutEvents` reads a list of entries too.

func publish(ctx context.Context, entries []types.PutEventsRequestEntry) {
	client := eventbridge.NewFromConfig(cfg)
	client.PutEvents(ctx, &eventbridge.PutEventsInput{Entries: entries})
}

func passedBus(ctx context.Context) {
	publish(ctx, []types.PutEventsRequestEntry{{EventBusName: aws.String("passed-bus")}})
}
