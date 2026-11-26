package main

import (
	"context"

	kafka "github.com/segmentio/kafka-go"
)

// The literal counterparts, outside the service's package.
func Created(ctx context.Context) {
	w := &kafka.Writer{Topic: "orders.created"}
	w.WriteMessages(ctx, kafka.Message{})
}

func Audit(ctx context.Context) {
	w := &kafka.Writer{Topic: "orders.audit"}
	w.WriteMessages(ctx, kafka.Message{})
}

func Events(ctx context.Context) {
	r := kafka.NewReader(kafka.ReaderConfig{Topic: "events.published"})
	r.ReadMessage(ctx)
}
