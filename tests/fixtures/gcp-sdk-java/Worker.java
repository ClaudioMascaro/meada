import com.google.cloud.pubsub.v1.Publisher;
import com.google.cloud.pubsub.v1.Subscriber;
import com.google.pubsub.v1.TopicName;
import com.google.pubsub.v1.SubscriptionName;
import com.google.cloud.storage.StorageOptions;
import com.google.cloud.storage.BlobId;
import com.google.cloud.storage.BlobInfo;
import com.google.cloud.secretmanager.v1.SecretManagerServiceClient;
import com.google.cloud.secretmanager.v1.SecretVersionName;

class Worker {
    void send() {
        var publisher = Publisher.newBuilder(TopicName.of("acme", "orders-java")).build();
        publisher.publish(message);
    }
    void receive() {
        var subscriber = Subscriber.newBuilder(SubscriptionName.of("acme", "orders-java"), handler).build();
        subscriber.startAsync();
    }
    void storage() {
        var storage = StorageOptions.newBuilder().build().getService();
        storage.create(BlobInfo.newBuilder(BlobId.of("uploads-java", unknownObject)).build(), data);
        storage.readAllBytes("uploads-java", unknownObject);
    }
    void token() {
        var secrets = SecretManagerServiceClient.create();
        secrets.accessSecretVersion(SecretVersionName.of("acme", "token-java", "latest"));
    }
    void hidden(Publisher Publisher, StorageOptions StorageOptions, SecretManagerServiceClient SecretManagerServiceClient) {
        var publisher = Publisher.newBuilder("hidden-topic").build();
        publisher.publish(message);
        var storage = StorageOptions.newBuilder().build().getService();
        storage.readAllBytes("hidden-bucket", "object");
        var secrets = SecretManagerServiceClient.create();
        secrets.accessSecretVersion("hidden-secret");
    }
}
