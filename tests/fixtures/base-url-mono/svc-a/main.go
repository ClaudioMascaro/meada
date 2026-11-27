package main

import (
	"context"

	"example.com/mono/svc-a/client"
	"example.com/mono/svc-a/config"
)

func main() {
	cfg, _ := config.Load()
	c := client.New(cfg.B.BaseURL)
	_, _ = c.Run(context.Background(), client.RunRequest{Name: "x"})
	_, _ = c.Cancel(context.Background(), "id")
}
