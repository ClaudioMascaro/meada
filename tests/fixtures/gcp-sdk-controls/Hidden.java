import com.google.cloud.pubsub.v1.Publisher;
class Hidden {
    static class Publisher { static Publisher newBuilder(String name) { return new Publisher(); } Publisher build() { return this; } void publish(Object data) {} }
    void hidden() { var client = Publisher.newBuilder("forbidden-type").build(); client.publish(data); }
}
