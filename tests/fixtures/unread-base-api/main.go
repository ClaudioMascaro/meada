package main

import "net/http"

func main() {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/orders", h)
	mux.HandleFunc("GET /orders", h)
	mux.HandleFunc("GET /v2/orders", h)
	mux.HandleFunc("GET /items", h)
	mux.HandleFunc("GET /api/orders/items", h)
	mux.HandleFunc("GET /api/items/export", h)
	mux.HandleFunc("GET /api/lineitems", h)
	http.ListenAndServe(":8080", mux)
}

func h(w http.ResponseWriter, r *http.Request) {}
