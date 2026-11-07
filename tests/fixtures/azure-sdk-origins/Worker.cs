using Azure.Messaging.ServiceBus;
using Azure.Messaging.EventHubs.Producer;
using Azure.Storage.Blobs;
using Azure.Security.KeyVault.Secrets;
class Worker {
 ServiceBusReceiver Receiver(string topic, string sub) { return new ServiceBusClient(connection).CreateReceiver(topic, sub); }
 async Task Run() {
  await Receiver("topic-first-csharp", "audit").ReceiveMessagesAsync(1);
  await Receiver("topic-second-csharp", "ops").ReceiveMessagesAsync(1);
 }
 async Task Typed(ServiceBusReceiver client) { await client.ReceiveMessagesAsync(1); }
 async Task Reassigned() { var client = Receiver("stale-csharp", "audit"); client = opaque; await client.ReceiveMessagesAsync(1); }
 ServiceBusReceiver Alternative(bool flag) { if(flag) return Receiver("unsafe-csharp", "audit"); return opaque; }
 async Task OpaquePath() { await Alternative(flag).ReceiveMessagesAsync(1); }
 async Task NameMatched() { await foreign.Receiver("matched-csharp", "audit").ReceiveMessagesAsync(1); }
 void HandleOnly() { new ServiceBusClient(connection).CreateReceiver("handle-only-csharp", "audit"); }
 async Task Unsupported() { await new ServiceBusClient(connection).CreateReceiver("unsupported-csharp").InventedReceive(); }
 async Task Unaffected() { await new ServiceBusClient(connection).CreateReceiver("unaffected-csharp").ReceiveMessageAsync(); }
 async Task Hidden(ServiceBusClient ServiceBusClient, EventHubProducerClient EventHubProducerClient, BlobServiceClient BlobServiceClient, SecretClient SecretClient) {
  await new ServiceBusClient(connection).CreateReceiver("hidden-csharp").ReceiveMessageAsync();
  await new EventHubProducerClient(connection, "hidden-hub-csharp").SendAsync(batch);
  await new BlobServiceClient(connection).GetBlobContainerClient("hidden-blob-csharp").UploadBlobAsync("object", data);
  await new SecretClient(vault, credential).GetSecretAsync("hidden-secret-csharp");
 }
}
