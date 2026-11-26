package other

import "context"

// A client of the same qualified name in another file: no declaration names it.
type Client struct{}

func (c *Client) Consume(ctx context.Context, topic string, ch chan int) error { return nil }

func Use(ctx context.Context) {
	c := &Client{}
	c.Consume(ctx, "orders.created", nil)
}
