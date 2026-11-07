using Amazon.SimpleNotificationService;
using Amazon.SimpleNotificationService.Model;

class HiddenConstructor {
    class AmazonSimpleNotificationServiceClient {
        public Task PublishAsync(PublishRequest request) { return null; }
    }
    async Task Send() {
        var client = new AmazonSimpleNotificationServiceClient();
        await client.PublishAsync(new PublishRequest { TopicArn = "hidden-constructor" });
    }
}
