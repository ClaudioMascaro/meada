package client

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
)

const runPath = "/run"

const cancelFormat = "/w/%s/cancel"

// RunRequest mirrors svc-b's request body by hand.
type RunRequest struct {
	Name string `json:"name"`
}

type Client struct {
	base string
	hc   *http.Client
}

func New(base string) *Client { return &Client{base: base, hc: http.DefaultClient} }

func (c *Client) send(ctx context.Context, method, path string, body any) (*http.Response, error) {
	b, err := json.Marshal(body)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, method, c.base+path, bytes.NewReader(b))
	if err != nil {
		return nil, err
	}
	return c.hc.Do(req)
}

func (c *Client) Run(ctx context.Context, r RunRequest) (*http.Response, error) {
	return c.send(ctx, http.MethodPost, runPath, r)
}

func (c *Client) Cancel(ctx context.Context, id string) (*http.Response, error) {
	return c.send(ctx, http.MethodPost, fmt.Sprintf(cancelFormat, url.PathEscape(id)), nil)
}
