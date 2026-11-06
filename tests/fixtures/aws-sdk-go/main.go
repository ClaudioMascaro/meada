package application

import (
    "context"
    "os"
    "github.com/aws/aws-sdk-go-v2/aws"
    "github.com/aws/aws-sdk-go-v2/service/s3"
)

func useBucket() {
    client := s3.NewFromConfig(cfg)
    client.PutObject(context.TODO(), &s3.PutObjectInput{Bucket: aws.String("uploads")})
    client.GetObject(context.TODO(), &s3.GetObjectInput{Bucket: aws.String("uploads")})
    client.GetObject(context.TODO(), &s3.GetObjectInput{Bucket: aws.String(os.Getenv("BUCKET"))})
}
