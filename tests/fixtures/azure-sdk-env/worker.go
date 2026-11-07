package worker
import "os"
import bus "github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"
func receive() { client, _ := bus.NewClient(namespace, credential, nil); receiver, _ := client.NewReceiverForSubscription(os.Getenv("TOPIC"), os.Getenv("SUB"), nil); receiver.ReceiveMessages(ctx, 1, nil) }
func queue() { client, _ := bus.NewClient(namespace, credential, nil); receiver, _ := client.NewReceiverForQueue(os.Getenv("QUEUE"), nil); receiver.ReceiveMessages(ctx, 1, nil) }
func literalTopic() { client, _ := bus.NewClient(namespace, credential, nil); receiver, _ := client.NewReceiverForSubscription("literal", os.Getenv("SUB"), nil); receiver.ReceiveMessages(ctx, 1, nil) }
func literalSub() { client, _ := bus.NewClient(namespace, credential, nil); receiver, _ := client.NewReceiverForSubscription(os.Getenv("TOPIC"), "fixed", nil); receiver.ReceiveMessages(ctx, 1, nil) }
func unread() { client, _ := bus.NewClient(namespace, credential, nil); receiver, _ := client.NewReceiverForSubscription(os.Getenv("TOPIC"), os.Getenv("UNREAD"), nil); receiver.ReceiveMessages(ctx, 1, nil) }

func literalQueue() { client, _ := bus.NewClient(namespace,credential,nil); receiver, _ := client.NewReceiverForQueue("unaffected-go",nil); receiver.ReceiveMessages(ctx,1,nil) }
