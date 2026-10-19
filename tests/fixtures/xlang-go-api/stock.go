package main

import (
	"os"
	"strings"

	"github.com/go-resty/resty/v2"
)

// Through the base URL of a resty client.
func stock() error {
	client := resty.New().SetBaseURL(strings.TrimSpace(os.Getenv("STOCK_URL")))
	_, err := client.R().Get("/api/stock")
	return err
}
