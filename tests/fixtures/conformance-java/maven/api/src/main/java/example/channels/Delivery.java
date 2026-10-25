package example.channels;

import io.nats.client.Connection;
import io.nats.client.Dispatcher;
import org.springframework.jms.annotation.JmsListener;
import org.springframework.jms.core.JmsTemplate;
import software.amazon.awssdk.services.sqs.SqsClient;
import software.amazon.awssdk.services.sqs.model.ReceiveMessageRequest;
import software.amazon.awssdk.services.sqs.model.SendMessageRequest;

// JMS, SQS and NATS. A polling consumer's handler is the method that names the queue, though
// the loop that receives runs in another.
public class Delivery {
    static final String QUEUE = "https://sqs.us-east-1.amazonaws.com/123456789012/shipments";

    private final JmsTemplate jms;
    private final SqsClient sqs;
    private final Connection nats;

    public Delivery(JmsTemplate jms, SqsClient sqs, Connection nats) {
        this.jms = jms;
        this.sqs = sqs;
        this.nats = nats;
    }

    public void notifyCustomer(String id) {
        jms.convertAndSend("notifications", id);
    }

    @JmsListener(destination = "notifications")
    public void onNotification(String message) {}

    public void ship(String id) {
        sqs.sendMessage(SendMessageRequest.builder().queueUrl(QUEUE).messageBody(id).build());
    }

    Object receive(String url) {
        return sqs.receiveMessage(ReceiveMessageRequest.builder().queueUrl(url).build());
    }

    public void shipments() {
        loop(receive(QUEUE));
    }

    void loop(Object messages) {}

    public void track(String id) {
        nats.publish("delivery.eu.tracked", id.getBytes());
    }

    public void watch(Dispatcher dispatcher) {
        dispatcher.subscribe("delivery.*.tracked", message -> {});
    }
}
