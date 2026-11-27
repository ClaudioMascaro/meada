package main

import (
	"context"
	"os"

	kafka "github.com/segmentio/kafka-go"
)

// A nested package reading the same variable: the chart runs its parent package only.
func Nested(ctx context.Context) {
	r := kafka.NewReader(kafka.ReaderConfig{Topic: os.Getenv("ORDERS_TOPIC")})
	r.ReadMessage(ctx)
}
