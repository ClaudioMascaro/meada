using System;
using Google.Cloud.PubSub.V1;
using Google.Cloud.Storage.V1;
using Google.Cloud.SecretManager.V1;
class Control {
    async Task<PublisherClient> Make(string name) { return await PublisherClient.CreateAsync(name); }
    async Task First() { var client = await Make("factory-csharp-a"); await client.PublishAsync(data); }
    async Task Second() { var client = await Make("factory-csharp-b"); await client.PublishAsync(data); }
    async Task EnvSend() { var client = await PublisherClient.CreateAsync(Environment.GetEnvironmentVariable("TOPIC_CSHARP")); await client.PublishAsync(data); }
    async Task EnvReceive() { var client = await SubscriberClient.CreateAsync(Environment.GetEnvironmentVariable("SUB_CSHARP")); await client.StartAsync(handler); }
    void EnvBucket() { var client = StorageClient.Create(); client.DownloadObject(Environment.GetEnvironmentVariable("BUCKET_CSHARP"), unknown, output); }
    void EnvSecret() { var client = SecretManagerServiceClient.Create(); client.AccessSecretVersion(Environment.GetEnvironmentVariable("SECRET_CSHARP")); }
    async Task<PublisherClient> MakeClient() { return await PublisherClient.CreateAsync("forbidden-hop"); }
    async Task Guessed(object unknown) { var client = await unknown.MakeClient(); await client.PublishAsync(data); }
    async Task<PublisherClient> Mixed(bool flag, PublisherClient unknown) { if (flag) return await PublisherClient.CreateAsync("forbidden-branch"); return unknown; }
    async Task Opaque(bool flag, PublisherClient unknown) { var client = await Mixed(flag, unknown); await client.PublishAsync(data); }
    async Task Reassigned(PublisherClient unknown) { var client = await PublisherClient.CreateAsync("forbidden-write"); client = unknown; await client.PublishAsync(data); }
    async Task Injected(PublisherClient client) { await client.PublishAsync(data); }
}
