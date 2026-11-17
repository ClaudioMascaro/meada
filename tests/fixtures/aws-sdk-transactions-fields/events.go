package application

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/eventbridge"
	"github.com/aws/aws-sdk-go-v2/service/eventbridge/types"
)

// `PutEvents` reads a list of entries too: an `EventBusName` written through another name
// (`reroute`, `other.Reroute`) leaves a list held alone its bus.

func heldBus(ctx context.Context) {
	client := eventbridge.NewFromConfig(cfg)
	entries := []types.PutEventsRequestEntry{{EventBusName: aws.String("held-bus")}}
	client.PutEvents(ctx, &eventbridge.PutEventsInput{Entries: entries})
}

func writtenBus(ctx context.Context) {
	client := eventbridge.NewFromConfig(cfg)
	entries := []types.PutEventsRequestEntry{{EventBusName: aws.String("written-bus-old")}}
	entries[0].EventBusName = aws.String("written-bus-new")
	client.PutEvents(ctx, &eventbridge.PutEventsInput{Entries: entries})
}

type route struct {
	EventBusName *string
	Entries      []types.PutEventsRequestEntry
}

func reroute(r *route) {
	r.EventBusName = aws.String("route")
	r.Entries = nil
}
