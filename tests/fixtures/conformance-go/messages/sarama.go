package messages

import (
	"context"

	"github.com/IBM/sarama"
)

func SaramaProduce() {
	producer, _ := sarama.NewSyncProducer([]string{"kafka:9092"}, nil)
	producer.SendMessage(&sarama.ProducerMessage{Topic: "orders", Value: sarama.StringEncoder("o")})
	message := &sarama.ProducerMessage{Topic: "payments"}
	producer.SendMessage(message)
}

func SaramaPartition() {
	consumer, _ := sarama.NewConsumer([]string{"kafka:9092"}, nil)
	consumer.ConsumePartition("refunds.eu", 0, sarama.OffsetNewest)
}

// The group's handler is the `ConsumeClaim` of the handler type.
type orderHandler struct{}

func (orderHandler) Setup(sarama.ConsumerGroupSession) error   { return nil }
func (orderHandler) Cleanup(sarama.ConsumerGroupSession) error { return nil }
func (orderHandler) ConsumeClaim(session sarama.ConsumerGroupSession, claim sarama.ConsumerGroupClaim) error {
	for message := range claim.Messages() {
		session.MarkMessage(message, "")
	}
	return nil
}

func SaramaGroup(ctx context.Context) {
	group, _ := sarama.NewConsumerGroup([]string{"kafka:9092"}, "orders", nil)
	handler := orderHandler{}
	group.Consume(ctx, []string{"orders", "payments"}, handler)
	group.Consume(ctx, []string{"invoices"}, &orderHandler{})
}
