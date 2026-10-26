package example.channels

import io.nats.client.Connection
import io.nats.client.Dispatcher
import org.springframework.jms.annotation.JmsListener
import org.springframework.jms.core.JmsTemplate
import software.amazon.awssdk.services.sqs.SqsClient
import software.amazon.awssdk.services.sqs.model.ReceiveMessageRequest
import software.amazon.awssdk.services.sqs.model.SendMessageRequest

private const val QUEUE = "https://sqs.us-east-1.amazonaws.com/123456789012/shipments"

// JMS, SQS and NATS. A polling consumer's handler is the function that names the queue, though
// the loop that receives runs in another.
class Delivery(
    private val jms: JmsTemplate,
    private val sqs: SqsClient,
    private val nats: Connection,
) {
    fun notifyCustomer(id: String) {
        jms.convertAndSend("notifications", id)
    }

    @JmsListener(destination = "notifications")
    fun onNotification(message: String) {}

    fun ship(id: String) {
        sqs.sendMessage(SendMessageRequest.builder().queueUrl(QUEUE).messageBody(id).build())
    }

    fun receive(url: String): Any = sqs.receiveMessage(ReceiveMessageRequest.builder().queueUrl(url).build())

    fun shipments() {
        loop(receive(QUEUE))
    }

    fun loop(messages: Any) {}

    fun track(id: String) {
        nats.publish("delivery.eu.tracked", id.toByteArray())
    }

    fun watch(dispatcher: Dispatcher) {
        dispatcher.subscribe("delivery.*.tracked") { }
    }
}
