import com.google.cloud.pubsub.v1.Publisher;
import com.google.cloud.pubsub.v1.Subscriber;
import com.google.cloud.storage.StorageOptions;
import com.google.cloud.secretmanager.v1.SecretManagerServiceClient;
class Control {
    Publisher make(String name) { return Publisher.newBuilder(name).build(); }
    void first() { var client = make("factory-java-a"); client.publish(data); }
    void second() { var client = make("factory-java-b"); client.publish(data); }
    void envSend() { var client = Publisher.newBuilder(System.getenv("TOPIC_JAVA")).build(); client.publish(data); }
    void envReceive() { var client = Subscriber.newBuilder(System.getenv("SUB_JAVA"), handler).build(); client.startAsync(); }
    void envBucket() { var client = StorageOptions.newBuilder().build().getService(); client.readAllBytes(System.getenv("BUCKET_JAVA"), unknown); }
    void envSecret() { var client = SecretManagerServiceClient.create(); client.accessSecretVersion(System.getenv("SECRET_JAVA")); }
    Publisher makeClient() { return Publisher.newBuilder("forbidden-hop").build(); }
    void guessed() { var unknown = opaqueReceiver(); var client = unknown.makeClient(); client.publish(data); }
    Publisher mixed(boolean flag, Publisher unknown) { if (flag) return Publisher.newBuilder("forbidden-branch").build(); return unknown; }
    void opaque(boolean flag, Publisher unknown) { var client = mixed(flag, unknown); client.publish(data); }
    void reassigned(Publisher unknown) { var client = Publisher.newBuilder("forbidden-write").build(); client = unknown; client.publish(data); }
    void injected(Publisher client) { client.publish(data); }
}
