package messages

import "context"

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
