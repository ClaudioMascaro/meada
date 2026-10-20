package client

import (
	"net/http"
	"os"

	"github.com/go-resty/resty/v2"
)

// A base URL read from the environment leads the URL as its variable.
func FromEnv() {
	http.Get(os.Getenv("API_URL") + "/env/get")
}

func FromLookup() {
	base, ok := os.LookupEnv("BILLING_URL")
	if !ok {
		base = "http://localhost:8080"
	}
	resty.New().SetBaseURL(base).R().Get("/env/lookup")
}
