package main

import (
	"context"

	"github.com/IBM/sarama"
)

// The web app (TypeScript) produces the topic; the handler type's ConsumeClaim handles it.
type shipments struct{}

func (shipments) Setup(sarama.ConsumerGroupSession) error   { return nil }
func (shipments) Cleanup(sarama.ConsumerGroupSession) error { return nil }
func (shipments) ConsumeClaim(session sarama.ConsumerGroupSession, claim sarama.ConsumerGroupClaim) error {
	for message := range claim.Messages() {
		applyShipment(string(message.Value))
		session.MarkMessage(message, "")
	}
	return nil
}

func applyShipment(id string) {
	_ = id
}

func consumeShipments(ctx context.Context) error {
	group, err := sarama.NewConsumerGroup([]string{"kafka:9092"}, "api", nil)
	if err != nil {
		return err
	}
	return group.Consume(ctx, []string{"orders.shipped"}, shipments{})
}
