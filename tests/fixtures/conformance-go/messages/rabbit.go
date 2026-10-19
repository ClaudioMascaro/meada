package messages

import (
	"context"

	amqp "github.com/rabbitmq/amqp091-go"
)

func channel() *amqp.Channel {
	conn, _ := amqp.Dial("amqp://rabbit:5672")
	ch, _ := conn.Channel()
	return ch
}

func PublishAll(ctx context.Context) {
	ch := channel()
	// The default exchange is the queue its key names.
	ch.PublishWithContext(ctx, "", "jobs", false, false, amqp.Publishing{Body: []byte("j")})
	ch.PublishWithContext(ctx, "events", "order.created", false, false, amqp.Publishing{})
	ch.Publish("events", "order.shipped", false, false, amqp.Publishing{})
}

func PublishUnknown(ctx context.Context, exchange string) {
	conn, _ := amqp.Dial("amqp://rabbit:5672")
	ch, _ := conn.Channel()
	ch.Publish(exchange, "order.lost", false, false, amqp.Publishing{})
}

func WorkJobs() {
	ch := channel()
	msgs, _ := ch.Consume("jobs", "", true, false, false, false, nil)
	for range msgs {
	}
}

func Audit() {
	ch := channel()
	ch.QueueBind("audit", "order.#", "events", false, nil)
	ch.Consume("audit", "", true, false, false, false, nil)
}
