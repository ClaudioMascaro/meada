package client

import "net/http"

type Client struct{ base string }

// Joins an unknown base with a path: only the real server fits it.
func (c *Client) Orders() (*http.Response, error) { return http.Get(c.base + "/orders") }
