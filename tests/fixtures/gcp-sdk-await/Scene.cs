using Google.Cloud.PubSub.V1;
using System.Threading.Tasks;
class Scene {
 static async Task Awaited() {var bus=await PublisherClient.CreateAsync(TopicName.FromProjectTopic("review","await-valid"));bus.PublishAsync(payload);}
 static async Task LateAwait() {var pending=PublisherClient.CreateAsync(TopicName.FromProjectTopic("review","late-valid"));var bus=await pending;bus.PublishAsync(payload);}
 static void Unawaited() {var pending=PublisherClient.CreateAsync(TopicName.FromProjectTopic("review","trap-pending"));pending.PublishAsync(payload);}
 static async Task DoubleAwait() {var bus=await PublisherClient.CreateAsync(TopicName.FromProjectTopic("review","trap-double"));var twice=await bus;twice.PublishAsync(payload);}
}
