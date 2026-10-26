package example.channels

import org.springframework.beans.factory.annotation.Value
import org.springframework.kafka.annotation.KafkaListener
import org.springframework.kafka.annotation.TopicPartition
import org.springframework.kafka.core.KafkaTemplate

const val CREATED = "orders.created"

// Spring Kafka: a topic from a constant, one from the module's configuration, and the names that
// cannot be evaluated. The template is a constructor property.
class Sales(
    private val kafka: KafkaTemplate<String, String>,
    @Value("\${channels.topic}") private val configured: String,
) {
    fun created(id: String) {
        kafka.send(CREATED, id)
    }

    fun paid(id: String) {
        kafka.send("orders.paid", id)
    }

    fun shipped(id: String) {
        emit(SHIPPED, id)
    }

    fun fromConfig(id: String) {
        this.kafka.send(configured, id)
    }

    fun computed(id: String) {
        kafka.send(System.getenv("ORDERS_TOPIC"), id)
    }

    fun suffixed(id: String, env: String) {
        kafka.send("orders-$env", id)
    }

    private fun emit(topic: String, id: String) {
        kafka.send(topic, id)
    }

    @KafkaListener(topics = [CREATED, "\${channels.topic}"], groupId = "sales")
    fun onSale(message: String) {}

    @KafkaListener(topics = ["\${channels.missing}"])
    fun onMissing(message: String) {}

    @KafkaListener(topics = arrayOf("orders.shipped", SHIPPED))
    fun onShipped(message: String) {}

    @KafkaListener(topicPartitions = [TopicPartition(topic = "orders.eu", partitions = ["0"])])
    fun onEurope(message: String) {}

    companion object {
        const val SHIPPED = "orders.shipped"
    }
}
