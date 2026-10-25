package example.channels;

import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.kafka.core.KafkaTemplate;

// Spring Kafka: a topic from a constant, one from the module's configuration, and one that
// cannot be evaluated.
public class Sales {
    static final String CREATED = "orders.created";

    private final KafkaTemplate<String, String> kafka;

    public Sales(KafkaTemplate<String, String> kafka) {
        this.kafka = kafka;
    }

    public void created(String id) {
        kafka.send(CREATED, id);
    }

    public void paid(String id) {
        kafka.send("orders.paid", id);
    }

    public void dynamic(String id) {
        kafka.send(System.getenv("ORDERS_TOPIC"), id);
    }

    @KafkaListener(topics = {CREATED, "${channels.topic}"})
    public void onSale(String message) {}

    @KafkaListener(topics = "${channels.missing}")
    public void onMissing(String message) {}
}
