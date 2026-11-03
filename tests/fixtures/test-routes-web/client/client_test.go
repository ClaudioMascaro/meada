package client

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

// A fake upstream the client's tests talk to: it serves nothing.
func stubUpstream() *httptest.Server {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /orders", func(w http.ResponseWriter, r *http.Request) {})
	return httptest.NewServer(mux)
}

func TestOrders(t *testing.T) {
	srv := stubUpstream()
	defer srv.Close()
	c := &Client{base: srv.URL}
	if _, err := c.Orders(); err != nil {
		t.Fatal(err)
	}
}
