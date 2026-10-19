package messages

import (
	"context"

	"github.com/nats-io/nats.go"
	"github.com/segmentio/kafka-go"
)

// Not the libraries': a transport of the repository that is told a topic, a subject or a stream.
type Outbox struct{}

func (Outbox) WriteMessages(ctx context.Context, topic string) {}
func (Outbox) Publish(subject string)                          {}
func (Outbox) XAdd(stream string)                              {}

func Shadowed(o *Outbox, ctx context.Context) {
	o.WriteMessages(ctx, "never.shadowed")
	o.Publish("never.subject")
	o.XAdd("never.stream")
}

var _ = nats.Version
var _ = kafka.TCP

// An import hidden by a parameter.
func ShadowedImport(nats *Outbox) {
	nats.Publish("never.import")
}

// A name bound again by `:=` in a block, in an `if` initializer and by a `range`.
func ShadowedBlock(ctx context.Context) {
	w := &kafka.Writer{}
	_ = w
	{
		w := &Outbox{}
		w.WriteMessages(ctx, "never.block")
	}
	if w := (&Outbox{}); w != nil {
		w.WriteMessages(ctx, "never.if")
	}
}

func ShadowedRange(ctx context.Context, all []*Outbox) {
	for _, w := range all {
		w.WriteMessages(ctx, "never.range")
	}
}

// A field typed by an interface of the repository.
type Sink interface {
	WriteMessages(ctx context.Context, topic string)
}

type Service struct{ sink Sink }

func (s *Service) Run(ctx context.Context) {
	s.sink.WriteMessages(ctx, "never.field")
}
