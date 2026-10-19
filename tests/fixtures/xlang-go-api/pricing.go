package main

import (
	"net/http"
	"os"
	"strings"
)

// Through a base URL variable, with a fallback and a trailing slash trimmed.
func priceOf(sku string) (*http.Response, error) {
	base := os.Getenv("WEB_URL")
	if base == "" {
		base = "http://localhost:3000"
	}
	return http.Get(strings.TrimSuffix(base, "/") + "/api/prices/" + sku)
}

// Through the web app's declared host.
func promotions() (*http.Response, error) {
	return http.Get("https://web.example.com/api/promotions")
}
