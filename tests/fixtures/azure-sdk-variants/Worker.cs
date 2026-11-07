using Azure.Messaging.ServiceBus;
using Azure.Messaging.EventHubs.Producer;
using Azure.Messaging.EventHubs.Consumer;
using Azure.Storage.Blobs;
using Azure.Security.KeyVault.Secrets;
class Worker {
 async Task Send() { var bus = new ServiceBusClient(connection); var sender = bus.CreateSender("orders-csharp"); await sender.SendMessageAsync(message); }
 async Task Receive() { var bus = new ServiceBusClient(connection); var receiver = bus.CreateReceiver("orders-csharp"); await receiver.ReceiveMessageAsync(); }
 async Task Subscribe() { var bus = new ServiceBusClient(connection); var receiver = bus.CreateReceiver("orders-csharp", "audit"); await receiver.ReceiveMessagesAsync(1); }
 async Task SendEvents() { var producer = new EventHubProducerClient(connection, "events-csharp"); await producer.SendAsync(batch); }
 async Task ReceiveEvents() { var consumer = new EventHubConsumerClient("group", connection, "events-csharp"); consumer.ReadEventsAsync(); }
 async Task Write() { var storage = new BlobServiceClient(connection); var container = storage.GetBlobContainerClient("uploads-csharp"); var blob = container.GetBlobClient(unknownObject); await blob.UploadAsync(data); }
 async Task Read() { var storage = new BlobServiceClient(connection); var blob = storage.GetBlobContainerClient("uploads-csharp").GetBlobClient(unknownObject); await blob.DownloadToAsync(stream); }
 async Task Secret() { var secret = new SecretClient(vault, credential); await secret.GetSecretAsync("token-csharp"); }

 async Task NamedReceiver() { await new ServiceBusClient(connection).CreateReceiver(topicName: "variant-csharp", subscriptionName: "audit").ReceiveMessageAsync(); }
 async Task NamedQueue() { await new ServiceBusClient(connection).CreateReceiver(queueName: "variant-csharp").ReceiveMessagesAsync(1); }
 void WriteSync() { new BlobServiceClient(connection).GetBlobContainerClient(blobContainerName: "variant-csharp").GetBlobClient(unknownObject).Upload(data); }
 void ReadSync() { new BlobServiceClient(connection).GetBlobContainerClient("variant-csharp").GetBlobClient(unknownObject).DownloadTo(stream); }
 void ContainerWrite() { new BlobServiceClient(connection).GetBlobContainerClient("variant-csharp").UploadBlob(unknownObject,data); }
 async Task ContainerWriteAsync() { await new BlobServiceClient(connection).GetBlobContainerClient("variant-csharp").UploadBlobAsync(unknownObject,data); }
 void SecretSync() { new SecretClient(vault,credential).GetSecret(name: "variant-csharp"); }
 async Task NamespaceProducer() { await new EventHubProducerClient(namespaceName,"variant-csharp",credential).SendAsync(batch); }
 async Task NamespaceConsumer() { new EventHubConsumerClient(group,namespaceName,"variant-csharp",credential).ReadEventsAsync(); }
}
