package messages

import (
	"os"

	"github.com/confluentinc/confluent-kafka-go/v2/kafka"
)

func ConfluentProduce() {
	p, _ := kafka.NewProducer(&kafka.ConfigMap{"bootstrap.servers": "kafka:9092"})
	topic := "orders"
	p.Produce(&kafka.Message{
		TopicPartition: kafka.TopicPartition{Topic: &topic, Partition: kafka.PartitionAny},
		Value:          []byte("o"),
	}, nil)
	partition := kafka.TopicPartition{Topic: &topic, Partition: kafka.PartitionAny}
	p.Produce(&kafka.Message{TopicPartition: partition}, nil)
	unknown := os.Getenv("EVENTS_TOPIC")
	p.Produce(&kafka.Message{TopicPartition: kafka.TopicPartition{Topic: &unknown}}, nil)
}

// The pointer is written through: the topic is not the one it was bound to.
func ConfluentDeref() {
	p, _ := kafka.NewProducer(&kafka.ConfigMap{"bootstrap.servers": "kafka:9092"})
	first := "first"
	tp := kafka.TopicPartition{Topic: &first}
	*tp.Topic = "second"
	p.Produce(&kafka.Message{TopicPartition: tp}, nil)
}

func ConfluentConsume() {
	c, _ := kafka.NewConsumer(&kafka.ConfigMap{"group.id": "orders"})
	c.Subscribe("payments", nil)
	c.SubscribeTopics([]string{"refunds.eu", "retries"}, nil)
	for {
		c.Poll(100)
	}
}

// A topic that starts with `^` is a regular expression: it names no topic.
func ConfluentPattern(c *kafka.Consumer) {
	c.SubscribeTopics([]string{"^audit-.*"}, nil)
}
