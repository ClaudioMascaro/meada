package config

// A tagged field of a named struct type nested in another, read from files of other packages.
type Config struct {
	Api Api
}

type Api struct {
	BaseURL string `env:"CROSS_URL"`
}

func Load() (*Config, error) { return &Config{}, nil }
