package written

import (
	"context"

	"github.com/segmentio/kafka-go"
)

// The field is written after the literal: the topic the writer has is not "a".
func Produce(ctx context.Context) {
	w := &kafka.Writer{Topic: "a"}
	w.Topic = "b"
	w.WriteMessages(ctx, kafka.Message{Value: []byte("v")})
}
