package main

import (
	"context"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/service/eventbridge"
	"github.com/aws/aws-sdk-go-v2/service/eventbridge/types"
	"github.com/aws/aws-sdk-go-v2/service/sns"
)

func bus(ctx context.Context) {
	cfg, _ := config.LoadDefaultConfig(ctx)
	client := eventbridge.NewFromConfig(cfg)
	client.PutEvents(ctx, &eventbridge.PutEventsInput{Entries: []types.PutEventsRequestEntry{types.PutEventsRequestEntry{Source: aws.String("app")}}})
}

func elided(ctx context.Context) {
	cfg, _ := config.LoadDefaultConfig(ctx)
	client := eventbridge.NewFromConfig(cfg)
	client.PutEvents(ctx, &eventbridge.PutEventsInput{Entries: []types.PutEventsRequestEntry{{Source: aws.String("app")}}})
}

func phone(ctx context.Context) {
	cfg, _ := config.LoadDefaultConfig(ctx)
	client := sns.NewFromConfig(cfg)
	client.Publish(ctx, &sns.PublishInput{PhoneNumber: aws.String("+15550100")})
}

func topic(ctx context.Context) {
	cfg, _ := config.LoadDefaultConfig(ctx)
	client := sns.NewFromConfig(cfg)
	client.Publish(ctx, &sns.PublishInput{TopicArn: aws.String("arn:aws:sns:us-east-1:123456789012:alerts-go")})
}

func unseen(ctx context.Context) {
	cfg, _ := config.LoadDefaultConfig(ctx)
	client := sns.NewFromConfig(cfg)
	client.Publish(ctx, input())
}

func helper(ctx context.Context) {
	cfg, _ := config.LoadDefaultConfig(ctx)
	client := sns.NewFromConfig(cfg)
	client.Publish(ctx, builder())
}
