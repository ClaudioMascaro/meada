package osk

import (
	"context"
	"net/http"
	"os"
)

type Client struct {
	os string
}

func New() *Client {
	return &Client{os: "linux"}
}

func (c *Client) CreateOrder(ctx context.Context) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, os.Getenv("API_URL")+"/orders", nil)
	if err != nil {
		return err
	}
	_, err = http.DefaultClient.Do(req)
	return err
}
