package main

import "net/http"

// The base URL read into a struct field by a tag (kelseyhightower/envconfig), nested.
type Settings struct {
	Web struct {
		URL string `envconfig:"STOCK_URL"`
	}
}

var settings Settings

func stockByTag() (*http.Response, error) {
	return http.Get(settings.Web.URL + "/api/stock")
}
