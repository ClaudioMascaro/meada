package example.channels;

import org.springframework.amqp.core.Binding;
import org.springframework.amqp.core.BindingBuilder;
import org.springframework.amqp.core.Queue;
import org.springframework.amqp.core.TopicExchange;
import org.springframework.amqp.rabbit.annotation.Exchange;
import org.springframework.amqp.rabbit.annotation.QueueBinding;
import org.springframework.amqp.rabbit.annotation.RabbitListener;
import org.springframework.amqp.rabbit.core.RabbitTemplate;

// Spring AMQP: a publish to a named exchange, one to the default exchange, the bindings of a
// listener and of a configuration.
public class Billing {
    private final RabbitTemplate rabbit;

    public Billing(RabbitTemplate rabbit) {
        this.rabbit = rabbit;
    }

    public void invoice(String id) {
        rabbit.convertAndSend("billing", "invoice.created", id);
    }

    public void refund(String id) {
        rabbit.convertAndSend("refunds", id);
    }

    public Binding audit() {
        return BindingBuilder.bind(new Queue("audit")).to(new TopicExchange("billing")).with("invoice.#");
    }

    @RabbitListener(bindings = @QueueBinding(
            value = @org.springframework.amqp.rabbit.annotation.Queue("invoices"),
            exchange = @Exchange("billing"),
            key = "invoice.*"))
    public void onInvoice(String message) {}

    @RabbitListener(queues = {"refunds", "audit"})
    public void onRefund(String message) {}
}
