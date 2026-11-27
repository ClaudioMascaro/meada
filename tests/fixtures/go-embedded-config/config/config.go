package config

import "example.com/acme/orders/pkg"

type (
	Kafka struct {
		Topics struct {
			Orders  string `env:"ORDERS_TOPIC"`
			Shipped string `envconfig:"SHIPPED_TOPIC"`
		}
	}
	Config    struct{ Kafka }
	Pointer   struct{ *Kafka }
	Qualified struct{ pkg.Kafka }
	Named     struct{ Kafka Kafka }
)
