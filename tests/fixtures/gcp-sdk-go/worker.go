package worker
import (
    "context"
    pubsub "cloud.google.com/go/pubsub/v2"
    legacy "cloud.google.com/go/pubsub"
    "cloud.google.com/go/storage"
    secretmanager "cloud.google.com/go/secretmanager/apiv1"
    secretmanagerpb "cloud.google.com/go/secretmanager/apiv1/secretmanagerpb"
)

func send() {
    client, _ := pubsub.NewClient(context.TODO(), "acme")
    client.Publisher("orders-go").Publish(context.TODO(), message)
}
func receive() {
    client, _ := pubsub.NewClient(context.TODO(), "acme")
    client.Subscriber("orders-go").Receive(context.TODO(), handle)
}
func compatible() {
    client, _ := legacy.NewClient(context.TODO(), "acme")
    client.Topic("orders-legacy").Publish(context.TODO(), message)
    client.Subscription("orders-legacy").Receive(context.TODO(), handle)
}
func objects() {
    client, _ := storage.NewClient(context.TODO())
    object := client.Bucket("uploads-go").Object(unknownObject)
    object.NewWriter(context.TODO())
    object.NewReader(context.TODO())
}
func token() {
    client, _ := secretmanager.NewClient(context.TODO())
    client.AccessSecretVersion(context.TODO(), &secretmanagerpb.AccessSecretVersionRequest{Name: "projects/acme/secrets/token-go/versions/latest"})
}
func hidden(pubsub interface{}, storage interface{}, secretmanager interface{}) {
    bus, _ := pubsub.NewClient(context.TODO(), "acme")
    bus.Publisher("hidden-topic").Publish(context.TODO(), message)
    store, _ := storage.NewClient(context.TODO())
    store.Bucket("hidden-bucket").Object("object").NewReader(context.TODO())
    secret, _ := secretmanager.NewClient(context.TODO())
    secret.AccessSecretVersion(context.TODO(), &secretmanagerpb.AccessSecretVersionRequest{Name: "hidden-secret"})
}
