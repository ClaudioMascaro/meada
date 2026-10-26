package example.channels

import org.springframework.amqp.core.Binding
import org.springframework.amqp.core.BindingBuilder
import org.springframework.amqp.core.ExchangeTypes
import org.springframework.amqp.core.Queue
import org.springframework.amqp.core.TopicExchange
import org.springframework.amqp.rabbit.annotation.Exchange
import org.springframework.amqp.rabbit.annotation.QueueBinding
import org.springframework.amqp.rabbit.annotation.RabbitListener
import org.springframework.amqp.rabbit.core.RabbitTemplate

// Spring AMQP: a publish to a named exchange, one to the default exchange, the bindings of a
// listener and of a configuration.
class Billing(private val rabbit: RabbitTemplate) {
    fun invoice(id: String) {
        rabbit.convertAndSend("billing", "invoice.created", id)
    }

    fun refund(id: String) {
        rabbit.convertAndSend("refunds", id)
    }

    fun auditBinding(): Binding =
        BindingBuilder.bind(Queue("audit")).to(TopicExchange("billing")).with("invoice.#")

    @RabbitListener(
        bindings = [
            QueueBinding(
                value = org.springframework.amqp.rabbit.annotation.Queue("invoices"),
                exchange = Exchange(value = "billing", type = ExchangeTypes.TOPIC),
                key = ["invoice.*"],
            ),
        ],
    )
    fun onInvoice(message: String) {}

    @RabbitListener(queues = ["refunds", "audit"])
    fun onRefund(message: String) {}
}
