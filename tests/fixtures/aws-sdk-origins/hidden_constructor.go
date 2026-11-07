package application
import (
    "context"
    "github.com/aws/aws-sdk-go-v2/aws"
    "github.com/aws/aws-sdk-go-v2/service/sns"
)
func hiddenConstructor(sns interface{}) {
    client := sns.NewFromConfig(cfg)
    client.Publish(context.TODO(), &sns.PublishInput{TopicArn: aws.String("hidden-constructor")})
}
