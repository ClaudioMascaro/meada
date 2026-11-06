package application
import (
    "context"
    environment "os"
    pointers "github.com/aws/aws-sdk-go-v2/aws"
    storage "github.com/aws/aws-sdk-go-v2/service/s3"
)
func use() {
    client := storage.NewFromConfig(cfg)
    client.PutObject(context.TODO(), &storage.PutObjectInput{Bucket: pointers.String(environment.Getenv("GO_BUCKET")), Key: pointers.String("row")})
    client.GetObject(context.TODO(), &storage.GetObjectInput{Bucket: pointers.String(environment.Getenv("GO_BUCKET")), Key: pointers.String("row")})
    request := &storage.GetObjectInput{Bucket: pointers.String("stale"), Key: pointers.String("row")}
    request.Bucket = pointers.String("changed")
    client.GetObject(context.TODO(), request)
}
