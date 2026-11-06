package application

import (
    "context"
    "github.com/aws/aws-sdk-go-v2/aws"
    "github.com/aws/aws-sdk-go-v2/service/sns"
    "github.com/aws/aws-sdk-go-v2/service/sqs"
)

func makeClient() *sns.Client { return sns.NewFromConfig(cfg) }
func positive() {
    client := makeClient()
    client.Publish(context.TODO(), &sns.PublishInput{TopicArn: aws.String("positive")})
}
func hidden(client *sns.Client) {
    client.Publish(context.TODO(), &sns.PublishInput{TopicArn: aws.String("hidden")})
}
type LocalFactory struct{}
func (f LocalFactory) makeClient() *sns.Client { return sns.NewFromConfig(cfg) }
func guessed(unknown interface{}) {
    client := unknown.makeClient()
    client.Publish(context.TODO(), &sns.PublishInput{TopicArn: aws.String("guessed")})
}
func declared(client *sqs.Client) {
    client.SendMessage(context.TODO(), &sqs.SendMessageInput{QueueUrl: aws.String("https://sqs.us-east-1.amazonaws.com/123/declared")})
}
