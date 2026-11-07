package worker
import (
 bus "github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"
 hub "github.com/Azure/azure-sdk-for-go/sdk/messaging/azeventhubs"
 blob "github.com/Azure/azure-sdk-for-go/sdk/storage/azblob"
 secrets "github.com/Azure/azure-sdk-for-go/sdk/security/keyvault/azsecrets"
)
func run() {
 client, err := bus.NewClient(namespace, credential, nil)
 sender, err := client.NewSender("orders-go", nil)
 receiver, err := client.NewReceiverForQueue("orders-go", nil)
 subscription, err := client.NewReceiverForSubscription("orders-go", "audit", nil)
 sender.SendMessage(ctx, message, nil)
 receiver.ReceiveMessages(ctx, 1, nil)
 subscription.ReceiveMessages(ctx, 1, nil)
 producer, err := hub.NewProducerClient(namespace, "events-go", credential, nil)
 consumer, err := hub.NewConsumerClient(namespace, "events-go", "group", credential, nil)
 partition, err := consumer.NewPartitionClient("0", nil)
 producer.SendEventDataBatch(ctx, batch, nil)
 partition.ReceiveEvents(ctx, 1, nil)
 storage, err := blob.NewClient(endpoint, credential, nil)
 storage.UploadBuffer(ctx, "uploads-go", object, data, nil)
 storage.DownloadBuffer(ctx, "uploads-go", object, data, nil)
 secret, err := secrets.NewClient(vault, credential, nil)
 secret.GetSecret(ctx, "token-go", "", nil)
}

func variants() {
 client, _ := bus.NewClientFromConnectionString(connection, nil)
 sender, _ := client.NewSender("variant-go", nil)
 sender.SendMessageBatch(ctx, batch, nil)
 producer, _ := hub.NewProducerClientFromConnectionString(connection, "variant-go", nil)
 producer.SendEventDataBatch(ctx, batch, nil)
 consumer, _ := hub.NewConsumerClientFromConnectionString(connection, "variant-go", group, nil)
 partition, _ := consumer.NewPartitionClient(unknownPartition, nil)
 partition.ReceiveEvents(ctx, 1, nil)
 first, _ := blob.NewClientWithNoCredential(endpoint, nil)
 first.UploadFile(ctx, "variant-go", unknownObject, file, nil)
 first.DownloadFile(ctx, "variant-go", unknownObject, file, nil)
 second, _ := blob.NewClientWithSharedKeyCredential(endpoint, credential, nil)
 second.UploadStream(ctx, "variant-go", unknownObject, stream, nil)
 second.DownloadStream(ctx, "variant-go", unknownObject, nil)
 third, _ := blob.NewClientFromConnectionString(connection, nil)
 third.UploadBuffer(ctx, "factory-go", unknownObject, data, nil)
 third.DownloadBuffer(ctx, "factory-go", unknownObject, data, nil)
}
