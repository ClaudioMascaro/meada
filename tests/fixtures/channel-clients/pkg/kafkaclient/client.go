package kafkaclient

import (
	"context"

	kafka "github.com/segmentio/kafka-go"
)

type Message struct{ Value []byte }

// Client consumes a topic into a channel.
type Client interface {
	Consume(ctx context.Context, topic string, ch chan Message) error
}

type reader struct{ brokers []string }

func New(brokers []string) Client { return &reader{brokers: brokers} }

// Consume reads the topic with a segmentio Reader.
func (r *reader) Consume(ctx context.Context, topic string, ch chan Message) error {
	rd := kafka.NewReader(kafka.ReaderConfig{Brokers: r.brokers, Topic: topic})
	m, err := rd.ReadMessage(ctx)
	if err != nil {
		return err
	}
	ch <- Message{Value: m.Value}
	return nil
}
