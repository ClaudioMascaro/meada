package api

// Client talks to the shared service.
type Client struct {
	Base string
}

// NewClient builds a client.
func NewClient() *Client {
	return &Client{Base: "https://api.example.com"}
}

// Create makes one record.
func (c *Client) Create(name string) string {
	return c.Base + "/" + name
}
