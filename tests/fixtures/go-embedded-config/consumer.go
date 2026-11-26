package orders

import "example.com/acme/orders/config"

type Service struct{ cfg *config.Config }

func consume(topic string) {}

func (svc *Service) Run() { consume(svc.cfg.Kafka.Topics.Orders) }
