import software.amazon.awssdk.services.sns.SnsClient;
import software.amazon.awssdk.services.sns.model.PublishRequest;
import software.amazon.awssdk.services.sqs.SqsClient;
import software.amazon.awssdk.services.sqs.model.SendMessageRequest;

class Origins {
    SnsClient make() { return SnsClient.create(); }
    void positive() {
        var client = make();
        client.publish(PublishRequest.builder().topicArn("arn:aws:sns:us-east-1:123:positive").build());
    }
    void hidden(SnsClient client) {
        client.publish(PublishRequest.builder().topicArn("hidden").build());
    }
    void guessed(Object unknown) {
        var client = unknown.make();
        client.publish(PublishRequest.builder().topicArn("guessed").build());
    }
    void declared(SqsClient client) {
        client.sendMessage(SendMessageRequest.builder().queueUrl("https://sqs.us-east-1.amazonaws.com/123/declared").build());
    }
}
