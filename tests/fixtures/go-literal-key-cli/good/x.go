package good

import (
	"context"
	"net/http"
	"os"
)

type Client struct {
	hc *http.Client
}

func New() *Client {
	return &Client{hc: http.DefaultClient}
}

func (c *Client) CreateOrder(ctx context.Context) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, os.Getenv("API_URL")+"/orders", nil)
	if err != nil {
		return err
	}
	_, err = c.hc.Do(req)
	return err
}
