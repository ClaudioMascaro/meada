package worker
import bus "github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"
func sender(name string) *bus.Sender { client, _ := bus.NewClient(namespace, credential, nil); result, _ := client.NewSender(name, nil); return result }
func receiver(topic string, sub string) *bus.Receiver { client, _ := bus.NewClient(namespace, credential, nil); result, _ := client.NewReceiverForSubscription(topic, sub, nil); return result }
func run() {
 sender("factory-first-go").SendMessage(ctx, message, nil)
 sender("factory-second-go").SendMessage(ctx, message, nil)
 receiver("topic-first-go", "audit").ReceiveMessages(ctx, 1, nil)
 receiver("topic-second-go", "ops").ReceiveMessages(ctx, 1, nil)
}
func typed(client *bus.Sender) { client.SendMessage(ctx, message, nil) }
func hidden(bus any) { client, _ := bus.NewClient(namespace, credential, nil); result, _ := client.NewSender("hidden-go", nil); result.SendMessage(ctx, message, nil) }
func reassigned() { client := sender("stale-go"); client = opaque; client.SendMessage(ctx, message, nil) }
func alternative(flag bool) *bus.Sender { if flag { return sender("unsafe-go") }; return opaque }
func opaquePath() { alternative(flag).SendMessage(ctx, message, nil) }
func nameMatched() { foreign.sender("matched-go").SendMessage(ctx, message, nil) }
func unsupported() { sender("unsupported-go").InventedSend(ctx, message, nil) }
func handleOnly() *bus.Sender { return sender("handle-only-go") }
func unaffected() { client, _ := bus.NewClient(namespace, credential, nil); result, _ := client.NewSender("unaffected-go", nil); result.SendMessage(ctx, message, nil) }

type Factory struct{}
func (f Factory) makeClient() *bus.Client { client, _ := bus.NewClient(namespace,credential,nil); return client }
func guessName(unknown interface{}) { client := unknown.makeClient(); result, _ := client.NewSender("matched-constructor",nil); result.SendMessage(ctx,message,nil) }
