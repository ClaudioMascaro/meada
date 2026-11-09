import software.amazon.awssdk.services.eventbridge.EventBridgeClient;
import software.amazon.awssdk.services.eventbridge.model.PutEventsRequest;
import software.amazon.awssdk.services.eventbridge.model.PutEventsRequestEntry;
import software.amazon.awssdk.services.sns.SnsClient;
import software.amazon.awssdk.services.sns.model.PublishRequest;

class Defaults {
    void bus() {
        var events = EventBridgeClient.create();
        events.putEvents(PutEventsRequest.builder().entries(
            PutEventsRequestEntry.builder().source("app").build()
        ).build());
    }

    void phone() {
        var sns = SnsClient.create();
        sns.publish(PublishRequest.builder().phoneNumber("+15550100").message("m").build());
    }

    void topic() {
        var sns = SnsClient.create();
        sns.publish(PublishRequest.builder().topicArn("arn:aws:sns:us-east-1:123456789012:alerts-java").message("m").build());
    }

    void unseen(PublishRequest template) {
        var sns = SnsClient.create();
        sns.publish(template.toBuilder().message("m").build());
    }
}
