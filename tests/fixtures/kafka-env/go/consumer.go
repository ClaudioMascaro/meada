package main

import (
	"context"

	kafka "github.com/segmentio/kafka-go"
)

// A reader whose topic is a tagged field of another file.
func Consume(ctx context.Context) {
	cfg := Load()
	r := kafka.NewReader(kafka.ReaderConfig{Topic: cfg.Topic})
	r.ReadMessage(ctx)
}

// A writer whose topic is the same tagged field.
func Produce(ctx context.Context) {
	cfg := Load()
	w := &kafka.Writer{Topic: cfg.Topic}
	w.WriteMessages(ctx, kafka.Message{Value: []byte("x")})
}
