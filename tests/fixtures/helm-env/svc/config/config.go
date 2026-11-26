package config

// The service's configuration, read from the environment.
type Config struct {
	Kafka struct {
		Topics struct {
			Orders string `env:"ORDERS_TOPIC"`
		}
	}
	Api Api
}

type Api struct {
	BaseURL string `env:"B_URL"`
}

func Load() (*Config, error) { return &Config{}, nil }
