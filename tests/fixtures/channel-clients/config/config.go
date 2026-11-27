package config

// The service's configuration, read from the environment.
type Config struct {
	Kafka struct {
		Topics struct {
			Orders string `env:"ORDERS_TOPIC"`
		}
	}
	Bus Bus
}

type Bus struct {
	Broker Broker
}

type Broker struct {
	Topics struct {
		Events string `envconfig:"EVENTS_TOPIC"`
	}
}

func NewConfig() (*Config, error) { return &Config{}, nil }
