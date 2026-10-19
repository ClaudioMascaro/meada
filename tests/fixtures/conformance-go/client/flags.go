package client

import (
	"flag"
	"net/http"

	"example.com/acme/shop/client/config"
)

// Writes through a selector: an assignment, the address handed to the flag package, a compound
// assignment. The field `Host` of a request is no write to the constant `Host`.
func Configure(r *http.Request) {
	config.Base = "https://flag.example.com"
	flag.StringVar(&config.Path, "path", "/default", "")
	config.Mode += "/v2"
	r.Host = "other.example.com"
}
