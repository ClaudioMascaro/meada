package controls
import (
 "context"
 "os"
 pubsub "cloud.google.com/go/pubsub/v2"
 "cloud.google.com/go/storage"
 secretmanager "cloud.google.com/go/secretmanager/apiv1"
 secretmanagerpb "cloud.google.com/go/secretmanager/apiv1/secretmanagerpb"
)
func makeTopic(name string) *pubsub.Publisher { client, _ := pubsub.NewClient(context.TODO(), "acme"); return client.Publisher(name) }
func first() { client := makeTopic("factory-go-a"); client.Publish(context.TODO(), data) }
func second() { client := makeTopic("factory-go-b"); client.Publish(context.TODO(), data) }
func envSend() { client, _ := pubsub.NewClient(context.TODO(), "acme"); client.Publisher(os.Getenv("TOPIC_GO")).Publish(context.TODO(), data) }
func envReceive() { client, _ := pubsub.NewClient(context.TODO(), "acme"); client.Subscriber(os.Getenv("SUB_GO")).Receive(context.TODO(), handler) }
func envBucket() { client, _ := storage.NewClient(context.TODO()); client.Bucket(os.Getenv("BUCKET_GO")).Object(unknown).NewReader(context.TODO()) }
func envSecret() { client, _ := secretmanager.NewClient(context.TODO()); client.AccessSecretVersion(context.TODO(), &secretmanagerpb.AccessSecretVersionRequest{Name: os.Getenv("SECRET_GO")}) }
type Factory struct{}
func (f Factory) makeClient() *pubsub.Client { client, _ := pubsub.NewClient(context.TODO(), "acme"); return client }
func guessed(unknown interface{}) { client := unknown.makeClient(); client.Publisher("forbidden-hop").Publish(context.TODO(), data) }
func mixed(flag bool, unknown *pubsub.Client) *pubsub.Client { if flag { client, _ := pubsub.NewClient(context.TODO(), "acme"); return client }; return unknown }
func opaque(flag bool, unknown *pubsub.Client) { client := mixed(flag, unknown); client.Publisher("forbidden-branch").Publish(context.TODO(), data) }
func reassigned(unknown *pubsub.Client) { client, _ := pubsub.NewClient(context.TODO(), "acme"); client = unknown; client.Publisher("forbidden-write").Publish(context.TODO(), data) }
func hidden(pubsub interface{}) { client, _ := pubsub.NewClient(context.TODO(), "acme"); client.Publisher("forbidden-parameter").Publish(context.TODO(), data) }
func injected(client *pubsub.Client) { client.Publisher("forbidden-injected").Publish(context.TODO(), data) }
