package service

import (
	"context"

	"example.com/acme/clients/config"
	"example.com/acme/clients/pkg/kafkaclient"
)

type Service struct {
	kafka kafkaclient.Client
	cfg   *config.Config
}

// Run consumes the configured orders topic through the client interface.
func (s *Service) Run(ctx context.Context) error {
	ch := make(chan kafkaclient.Message)
	return s.kafka.Consume(ctx, s.cfg.Kafka.Topics.Orders, ch)
}

// Listen consumes the topic its caller names.
func (s *Service) Listen(ctx context.Context, topic string) error {
	ch := make(chan kafkaclient.Message)
	return s.kafka.Consume(ctx, topic, ch)
}

func (s *Service) Start(ctx context.Context) error {
	return s.Listen(ctx, "orders.audit")
}
