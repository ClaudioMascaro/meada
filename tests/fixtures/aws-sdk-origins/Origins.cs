using Amazon.SimpleNotificationService;
using Amazon.SimpleNotificationService.Model;
using Amazon.SQS;
using Amazon.SQS.Model;

class Origins {
    AmazonSimpleNotificationServiceClient Make() { return new AmazonSimpleNotificationServiceClient(); }
    async Task Positive() {
        var client = Make();
        await client.PublishAsync(new PublishRequest { TopicArn = "positive" });
    }
    async Task Hidden(AmazonSimpleNotificationServiceClient client) {
        await client.PublishAsync(new PublishRequest { TopicArn = "hidden" });
    }
    async Task Guessed(object unknown) {
        var client = unknown.Make();
        await client.PublishAsync(new PublishRequest { TopicArn = "guessed" });
    }
    async Task Declared(IAmazonSQS client) {
        await client.SendMessageAsync(new SendMessageRequest { QueueUrl = "https://sqs.us-east-1.amazonaws.com/123/declared" });
    }
}
