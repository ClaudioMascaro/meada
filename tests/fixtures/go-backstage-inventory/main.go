package main

import "net/http"

func main() {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /clusters", clusters)
	http.ListenAndServe(":8080", mux)
}

func clusters(w http.ResponseWriter, r *http.Request) {}
