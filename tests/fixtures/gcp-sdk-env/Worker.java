import com.google.cloud.pubsub.v1.Publisher;
import com.google.cloud.pubsub.v1.Subscriber;
class Worker {
    void publishJava() { var publisher = Publisher.newBuilder(System.getenv("TOPIC_ID")).build(); publisher.publish(data); }
    void receiveJava() { var subscriber = Subscriber.newBuilder(System.getenv("SUB_ID"), handler).build(); subscriber.startAsync(); }
}
