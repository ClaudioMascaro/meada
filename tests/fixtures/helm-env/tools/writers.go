package main

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	kafka "github.com/segmentio/kafka-go"
)

// The literal counterparts, outside the chart's package.
func Created(ctx context.Context) {
	w := &kafka.Writer{Topic: "orders.created"}
	w.WriteMessages(ctx, kafka.Message{})
}

func V2(ctx context.Context) {
	w := &kafka.Writer{Topic: "orders.v2"}
	w.WriteMessages(ctx, kafka.Message{})
}

func Audit(ctx context.Context) {
	client := s3.NewFromConfig(aws.Config{})
	client.PutObject(ctx, &s3.PutObjectInput{Bucket: aws.String("audit-bucket"), Key: aws.String("row")})
}
