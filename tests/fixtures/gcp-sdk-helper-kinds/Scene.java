import com.google.cloud.pubsub.v1.Publisher;
import com.google.cloud.pubsub.v1.Subscriber;
import com.google.pubsub.v1.TopicName;
import com.google.pubsub.v1.SubscriptionName;
import com.google.cloud.storage.StorageOptions;
import com.google.cloud.secretmanager.v1.SecretManagerServiceClient;
class Scene {
  Publisher make(Object name) { return Publisher.newBuilder(name).build(); }
  void Correct() { var client=make(TopicName.of("p","kind-java")); client.publish(data); }
  void Wrong() { var client=make(SubscriptionName.of("p","kind-java")); client.publish(data); }
  void WrongReceive() { var client=Subscriber.newBuilder(TopicName.of("p","wrong-java"),handler).build(); client.startAsync(); }
  void WrongBucket() { var client=StorageOptions.newBuilder().build().getService(); client.readAllBytes(TopicName.of("p","wrong-java"),object); }
  void WrongSecret() { var client=SecretManagerServiceClient.create(); client.accessSecretVersion(TopicName.of("p","wrong-java")); }
}
