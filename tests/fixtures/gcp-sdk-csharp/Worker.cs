using Google.Cloud.PubSub.V1;
using Google.Cloud.Storage.V1;
using Google.Cloud.SecretManager.V1;

class Worker {
    async Task Send() {
        var publisher = await PublisherClient.CreateAsync(new TopicName("acme", "orders-csharp"));
        await publisher.PublishAsync(message);
    }
    async Task Receive() {
        var subscriber = await SubscriberClient.CreateAsync(SubscriptionName.FromProjectSubscription("acme", "orders-csharp"));
        await subscriber.StartAsync(handler);
    }
    void Objects() {
        var storage = StorageClient.Create();
        storage.UploadObject("uploads-csharp", unknownObject, null, data);
        storage.DownloadObject("uploads-csharp", unknownObject, output);
    }
    async Task ObjectsAsync() {
        var storage = StorageClient.Create();
        await storage.UploadObjectAsync(bucket: "uploads-csharp", objectName: unknownObject, contentType: null, source: data);
        await storage.DownloadObjectAsync(bucket: "uploads-csharp", objectName: unknownObject, destination: output);
    }
    void Token() {
        var secrets = SecretManagerServiceClient.Create();
        secrets.AccessSecretVersion(new SecretVersionName("acme", "token-csharp", "latest"));
    }
    async Task TokenAsync() {
        var secrets = SecretManagerServiceClient.Create();
        await secrets.AccessSecretVersionAsync(name: "projects/acme/secrets/token-csharp/versions/latest");
    }
    async Task Hidden(object PublisherClient, object StorageClient, object SecretManagerServiceClient) {
        var publisher = await PublisherClient.CreateAsync("hidden-topic");
        await publisher.PublishAsync(message);
        var storage = StorageClient.Create();
        storage.DownloadObject("hidden-bucket", "object", output);
        var secrets = SecretManagerServiceClient.Create();
        secrets.AccessSecretVersion("hidden-secret");
    }
}
