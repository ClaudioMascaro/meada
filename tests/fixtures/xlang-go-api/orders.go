package main

import "net/http"

func router() *http.ServeMux {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /orders/{id}", status)
	mux.HandleFunc("POST /orders/reserve", reserve)
	return mux
}

func status(w http.ResponseWriter, r *http.Request) {
	priceOf(r.PathValue("id"))
	stock()
}

func reserve(w http.ResponseWriter, r *http.Request) {
	promotions()
	audit()
}
