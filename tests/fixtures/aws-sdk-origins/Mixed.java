import software.amazon.awssdk.services.sns.SnsClient;
import software.amazon.awssdk.services.sns.model.PublishRequest;

class Mixed {
    SnsClient mixed(boolean choose, SnsClient opaque) {
        if (choose) return SnsClient.create();
        return opaque;
    }
    void send(boolean choose, SnsClient opaque) {
        var client = mixed(choose, opaque);
        client.publish(PublishRequest.builder().topicArn("mixed-return").build());
    }
}
