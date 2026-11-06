import software.amazon.awssdk.services.sns.SnsClient;
import software.amazon.awssdk.services.sns.model.PublishRequest;

class HiddenConstructor {
    static class SnsClient {
        static SnsClient create() { return new SnsClient(); }
        void publish(PublishRequest request) {}
    }
    void send() {
        var client = SnsClient.create();
        client.publish(PublishRequest.builder().topicArn("hidden-constructor").build());
    }
}
