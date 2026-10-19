package client

import (
	"net/http"

	"github.com/go-resty/resty/v2"
)

// A resty request carries the base URL of the client that issued it.
func Rest(body map[string]string) {
	client := resty.New().SetBaseURL("https://billing.example.com").SetTimeout(5)
	client.R().Get("/invoices")
	client.R().SetBody(body).SetHeader("X-Id", "1").Post("/invoices")
	client.R().Put("/invoices/1")
	client.R().Patch("/invoices/1")
	client.R().Delete("/invoices/1")
	client.R().Head("/invoices")
	client.R().Options("/invoices")
	client.R().Execute(http.MethodGet, "/executed")
	client.R().Execute("POST", "/executed")
	req := client.R()
	req.Get("/stored")
	// An absolute URL keeps its own host.
	client.R().Get("https://other.example.com/absolute")
}

// The base set by a statement of its own, and no base at all.
func Separate() {
	ledger := resty.New()
	ledger.SetBaseURL("https://ledger.example.com")
	ledger.R().Get("/entries")
	bare := resty.New()
	bare.R().Get("https://bare.example.com/plain")
}

// A client written as a type, with no constructor in sight, has no base.
func Typed(c *resty.Client, r *resty.Request) {
	c.R().Get("/typed")
	r.Get("/typed-request")
}

type Gateway struct {
	rest  *resty.Client
	plain *http.Client
}

func (g *Gateway) Items() {
	g.rest.R().Get("/items")
	g.plain.Get("https://api.example.com/plain")
}

// A client a function returns carries its base.
func newClient() *resty.Client {
	return resty.New().SetBaseURL("https://returned.example.com")
}

func Returned() {
	newClient().R().Get("/returned")
}

// A client given two bases has an unknown one.
func Reused() {
	shared := resty.New().SetBaseURL("https://first.example.com")
	shared.SetBaseURL("https://second.example.com")
	shared.R().Get("/reused")
}

// A resty-looking chain on something else is no request.
func Other(store *Cache) {
	store.R().Get("/not-resty")
}

// A wrapper over a request.
func fetchJSON(c *resty.Client, path string) {
	c.R().Get(path)
}

func Wrapper() {
	c := resty.New().SetBaseURL("https://billing.example.com")
	fetchJSON(c, "/wrapped-rest")
}

// Two clients one factory makes do not share a base.
func plainClient() *resty.Client {
	return resty.New()
}

func Factory() {
	a := plainClient()
	a.SetBaseURL("https://a.example.com")
	b := plainClient()
	b.R().Get("/from-b")
	a.R().Get("/from-a")
}

// A wrapper given a traced client and one whose origin is not traced has an unknown base.
func fetchBase(c *resty.Client, path string) {
	c.R().Get(path)
}

func Mixed(u *resty.Client) {
	fetchBase(resty.New().SetBaseURL("https://m.example.com"), "/m1")
	fetchBase(u, "/m2")
}

// A client assigned again is not traced: its request is no consumer.
func Reassigned() {
	c := resty.New().SetBaseURL("https://first.example.com")
	c = resty.New()
	c.R().Get("/reassigned")
}
