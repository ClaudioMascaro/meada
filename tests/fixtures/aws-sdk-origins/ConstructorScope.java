import software.amazon.awssdk.services.sns.SnsClient;
import software.amazon.awssdk.services.sns.model.PublishRequest;

class ConstructorScope {
    final SnsClient given = SnsClient.create();
    final SnsClient client;
    ConstructorScope(SnsClient given) { client = given; }
    void use() {
        client.publish(PublishRequest.builder().topicArn("hidden-constructor-parameter").build());
    }
}
