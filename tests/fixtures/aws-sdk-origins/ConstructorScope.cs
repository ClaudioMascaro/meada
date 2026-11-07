using Amazon.SimpleNotificationService;
using Amazon.SimpleNotificationService.Model;

class ConstructorScope {
    readonly AmazonSimpleNotificationServiceClient given = new AmazonSimpleNotificationServiceClient();
    readonly AmazonSimpleNotificationServiceClient client;
    ConstructorScope(AmazonSimpleNotificationServiceClient given) { client = given; }
    async Task Use() {
        await client.PublishAsync(new PublishRequest { TopicArn = "hidden-constructor-parameter" });
    }
}
