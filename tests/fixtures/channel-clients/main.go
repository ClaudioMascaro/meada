package main

import (
	"example.com/acme/clients/config"
	"example.com/acme/flow"
)

func main() {
	cfg, err := config.NewConfig()
	if err != nil {
		return
	}
	flow.NewProducer(flow.ProducerConfig{TopicName: cfg.Bus.Broker.Topics.Events})
}
