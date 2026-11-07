package worker
import "os"
import bus "github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"
func run() {
 client, _ := bus.NewClient(namespace,credential,nil)
 queue, _ := client.NewReceiverForQueue(os.Getenv("BUS"),nil)
 subscription, _ := client.NewReceiverForSubscription(os.Getenv("BUS"),"audit",nil)
 queue.ReceiveMessages(ctx,1,nil)
 subscription.ReceiveMessages(ctx,1,nil)
}
