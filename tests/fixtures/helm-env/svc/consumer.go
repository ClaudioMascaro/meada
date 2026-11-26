package main

import (
	"context"
	"net/http"
	"os"

	"example.com/acme/svc/config"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	kafka "github.com/segmentio/kafka-go"
)

// A reader whose topic is a tagged field inside the configuration's anonymous structs.
func Consume(ctx context.Context) {
	cfg, err := config.Load()
	if err != nil {
		return
	}
	r := kafka.NewReader(kafka.ReaderConfig{Topic: cfg.Kafka.Topics.Orders})
	r.ReadMessage(ctx)
}

// An HTTP call on a tagged base URL.
func Fetch() {
	cfg, err := config.Load()
	if err != nil {
		return
	}
	http.Get(cfg.Api.BaseURL + "/orders")
}

// A bucket read from the environment.
func Read(ctx context.Context) {
	client := s3.NewFromConfig(aws.Config{})
	client.GetObject(ctx, &s3.GetObjectInput{Bucket: aws.String(os.Getenv("BUCKET")), Key: aws.String("row")})
}
