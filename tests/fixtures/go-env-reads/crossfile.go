package main

import (
	"net/http"

	"example.com/goenv/probes/config"
)

// The base URL comes from a tagged field of another package, reached through `Load`'s result
// type and the field's type.
type Cross struct{ base string }

func NewCross(base string) *Cross { return &Cross{base: base} }

func crossFromConfig() *Cross {
	loaded, err := config.Load()
	if err != nil {
		return nil
	}
	return NewCross(loaded.Api.BaseURL)
}

func (c *Cross) send(method, path string) (*http.Response, error) {
	req, err := http.NewRequest(method, c.base+path, nil)
	if err != nil {
		return nil, err
	}
	return http.DefaultClient.Do(req)
}

func (c *Cross) Get() (*http.Response, error) { return c.send("GET", "/cross") }
