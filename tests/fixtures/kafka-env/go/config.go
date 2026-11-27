package main

// The topic the consumer reads, from the environment.
type Config struct {
	Topic string `env:"ORDERS_TOPIC"`
}

func Load() *Config { return &Config{} }
