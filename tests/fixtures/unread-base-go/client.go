package client

import (
	"net/http"
	"os"
)

type Client struct{}

func (c *Client) Inline() (*http.Response, error) { return http.Get(os.Getenv("API_URL") + "/api/orders") }

func (c *Client) InlineOther() (*http.Response, error) { return http.Get(os.Getenv("OTHER_URL") + "/v2/orders") }

func (c *Client) baseV2() string { return os.Getenv("API_URL") + "/v2" }

func (c *Client) ViaMethodPrefix() (*http.Response, error) { return http.Get(c.baseV2() + "/orders") }
