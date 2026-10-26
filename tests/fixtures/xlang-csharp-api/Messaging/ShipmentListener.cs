using Confluent.Kafka;

namespace Api.Messaging;

// The web app (TypeScript, kafkajs) produces the topic. The consumer polls, so the method that
// subscribes handles what arrives.
public class ShipmentListener
{
    private readonly IConsumer<string, string> _consumer;

    public ShipmentListener(IConsumer<string, string> consumer)
    {
        _consumer = consumer;
    }

    public void OnShipment()
    {
        _consumer.Subscribe("orders.shipped");
    }
}
