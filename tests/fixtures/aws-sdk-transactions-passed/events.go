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

func changedBus(ctx context.Context) {
	entries := []types.PutEventsRequestEntry{{EventBusName: aws.String("bus-old")}}
	entries[0].EventBusName = aws.String("bus-new")
	publish(ctx, entries)
}

// An entry with no bus goes to `default` (ADR 0026), passed as it would be written in place.
func passedDefault(ctx context.Context) {
	publish(ctx, []types.PutEventsRequestEntry{{Source: aws.String("app")}})
}
