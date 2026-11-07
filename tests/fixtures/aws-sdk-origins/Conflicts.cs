using Amazon.SimpleNotificationService;
using Amazon.SimpleNotificationService.Model;

class Conflicts {
    AmazonSimpleNotificationServiceClient client;
    async Task HiddenField() {
        var client = new AmazonSimpleNotificationServiceClient();
        await this.client.PublishAsync(new PublishRequest { TopicArn = "hidden-field" });
    }
    AmazonSimpleNotificationServiceClient Mixed(bool choose, AmazonSimpleNotificationServiceClient opaque) {
        if (choose) return new AmazonSimpleNotificationServiceClient();
        return opaque;
    }
    async Task UnknownReturn(bool choose, AmazonSimpleNotificationServiceClient opaque) {
        var client = Mixed(choose, opaque);
        await client.PublishAsync(new PublishRequest { TopicArn = "mixed-return" });
    }
}
