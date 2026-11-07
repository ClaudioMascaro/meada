package application
import (
    "context"
    "github.com/aws/aws-sdk-go-v2/aws"
    "github.com/aws/aws-sdk-go-v2/service/sns"
)
func mixed(choose bool, opaque *sns.Client) *sns.Client {
    if choose { return sns.NewFromConfig(cfg) }
    return opaque
}
func mixedSend(choose bool, opaque *sns.Client) {
    client := mixed(choose, opaque)
    client.Publish(context.TODO(), &sns.PublishInput{TopicArn: aws.String("mixed-return")})
}
