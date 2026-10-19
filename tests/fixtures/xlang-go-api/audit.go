package main

import (
	"net/http"
	"os"
	"strings"
)

// Through a base URL variable read with LookupEnv.
func audit() (*http.Response, error) {
	base, _ := os.LookupEnv("WEB_URL")
	return http.Post(strings.TrimRight(base, "/")+"/api/audit", "application/json", nil)
}
