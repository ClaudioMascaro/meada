using Google.Cloud.PubSub.V1;
using Google.Cloud.Storage.V1;
using Google.Cloud.SecretManager.V1;
class Scene {
  async Task<PublisherClient> Make(object name) { return await PublisherClient.CreateAsync(name); }
  async Task Correct() { var client=await Make(new TopicName("p","kind-csharp")); await client.PublishAsync(data); }
  async Task Wrong() { var client=await Make(new SubscriptionName("p","kind-csharp")); await client.PublishAsync(data); }
  async Task WrongReceive() { var client=await SubscriberClient.CreateAsync(TopicName.FromProjectTopic("p","wrong-csharp")); await client.StartAsync(handler); }
  void WrongBucket() { var client=StorageClient.Create(); client.DownloadObject(TopicName.FromProjectTopic("p","wrong-csharp"),object,stream); }
  void WrongSecret() { var client=SecretManagerServiceClient.Create(); client.AccessSecretVersion(TopicName.FromProjectTopic("p","wrong-csharp")); }
}
