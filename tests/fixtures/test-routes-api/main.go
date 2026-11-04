package main

import "net/http"

func main() {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /orders", func(w http.ResponseWriter, r *http.Request) {})
	http.ListenAndServe(":8080", mux)
}
