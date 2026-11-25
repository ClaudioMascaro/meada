package config

// The client's settings: the base URL of svc-b comes from a tagged field of a nested struct.
type Config struct {
	B B
}

type B struct {
	BaseURL string `env:"B_BASE_URL"`
}

func Load() (*Config, error) { return &Config{}, nil }
