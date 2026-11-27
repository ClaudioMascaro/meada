package client

import (
	"net/http"
	"os"
)

type Endpoint struct {
	URL string `env:"ENDPOINT_URL"`
}
type Config struct{ Endpoint }
type Sender struct{ endpoint string }

func NewSender(cfg *Config) *Sender { return &Sender{endpoint: cfg.Endpoint.URL} }
func (s *Sender) postBase()         { http.NewRequest("POST", s.endpoint, nil) }
func postRoot()                     { http.NewRequest("POST", os.Getenv("ENDPOINT_URL")+"/", nil) }
