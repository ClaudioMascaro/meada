using System.Collections.Generic;
using System.Threading.Tasks;
using Amazon.EventBridge;
using Amazon.EventBridge.Model;
using Amazon.SimpleNotificationService;
using Amazon.SimpleNotificationService.Model;

class Defaults {
    async Task Bus() {
        var events = new AmazonEventBridgeClient();
        await events.PutEventsAsync(new PutEventsRequest {
            Entries = new List<PutEventsRequestEntry> { new PutEventsRequestEntry { Source = "app" } }
        });
    }

    async Task Phone() {
        var sns = new AmazonSimpleNotificationServiceClient();
        await sns.PublishAsync(new PublishRequest { PhoneNumber = "+15550100", Message = "m" });
    }

    async Task Topic() {
        var sns = new AmazonSimpleNotificationServiceClient();
        await sns.PublishAsync(new PublishRequest { TopicArn = "arn:aws:sns:us-east-1:123456789012:alerts-cs", Message = "m" });
    }
}
