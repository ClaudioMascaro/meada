package api

import (
	"context"
	"net/http"

	"example.com/types/svc"
)

// Handler declares the request struct; the context and the request of the standard library
// are types of external packages.
func Handler(ctx context.Context, r *http.Request) {
	var req svc.Req
	_ = req
	_ = ctx
	_ = r
}
