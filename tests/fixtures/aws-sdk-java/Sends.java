import software.amazon.awssdk.services.sns.SnsClient;
import software.amazon.awssdk.services.sns.model.PublishRequest;

class Sends {
    void send() {
        var sns = SnsClient.builder().build();
        sns.publish(PublishRequest.builder().topicArn("arn:aws:sns:us-east-1:123:alerts").message("hello").build());
    }
}
