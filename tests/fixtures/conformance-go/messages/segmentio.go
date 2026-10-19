package messages

import (
	"context"
	"os"

	"github.com/segmentio/kafka-go"
)

const region = "eu"

// A writer that names the topic: the messages do not.
func ProduceAll(ctx context.Context) {
	orders := &kafka.Writer{Addr: kafka.TCP("kafka:9092"), Topic: "orders"}
	orders.WriteMessages(ctx, kafka.Message{Value: []byte("o")})
	payments := kafka.Writer{Topic: "payments." + region}
	payments.WriteMessages(ctx, kafka.Message{Value: []byte("p")})
}

// A writer with no topic: each message names its own.
func ProduceMessages(ctx context.Context) {
	bare := &kafka.Writer{Addr: kafka.TCP("kafka:9092")}
	bare.WriteMessages(ctx,
		kafka.Message{Topic: "audits", Value: []byte("a")},
		kafka.Message{Topic: "audits." + region, Value: []byte("a")},
	)
	bare.WriteMessages(ctx, []kafka.Message{{Topic: "retries"}, {Topic: "invoices"}}...)
}

// A writer a function made: its topic is not in sight, so the message names it.
func newWriter() *kafka.Writer {
	return &kafka.Writer{Addr: kafka.TCP("kafka:9092"), Topic: "elsewhere"}
}

func ProduceMade(ctx context.Context) {
	w := newWriter()
	w.WriteMessages(ctx, kafka.Message{Topic: "refunds"})
}

func emit(ctx context.Context, w *kafka.Writer, topic string) {
	w.WriteMessages(ctx, kafka.Message{Topic: topic})
}

func EmitRefund(ctx context.Context) {
	w := &kafka.Writer{Addr: kafka.TCP("kafka:9092")}
	emit(ctx, w, "refund.requested")
}

func ProduceUnknown(ctx context.Context) {
	w := &kafka.Writer{Topic: os.Getenv("EVENTS_TOPIC")}
	w.WriteMessages(ctx, kafka.Message{Value: []byte("u")})
}

// The reader names the topic, or the topics of its group; the loop sits elsewhere.
func ConsumeOrders() *kafka.Reader {
	return kafka.NewReader(kafka.ReaderConfig{Brokers: []string{"kafka:9092"}, Topic: "orders", GroupID: "orders"})
}

func ConsumeRefunds() *kafka.Reader {
	return kafka.NewReader(kafka.ReaderConfig{GroupID: "refunds", GroupTopics: []string{"refunds.eu", "retries", "invoices"}})
}

func ConsumeUnknown() *kafka.Reader {
	return kafka.NewReader(kafka.ReaderConfig{Topic: os.Getenv("EVENTS_TOPIC")})
}

func ReadLoop(ctx context.Context, r *kafka.Reader) {
	for {
		if _, err := r.ReadMessage(ctx); err != nil {
			return
		}
	}
}
