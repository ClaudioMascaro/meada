package worker
import ("os"; bus "cloud.google.com/go/pubsub/v2")
func publishGo() { client, err := bus.NewClient(ctx, "p"); client.Publisher(os.Getenv("TOPIC_ID")).Publish(ctx, data) }
func receiveGo() { client, err := bus.NewClient(ctx, "p"); client.Subscriber(os.Getenv("SUB_ID")).Receive(ctx, handler) }
