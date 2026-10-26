using System;
using NATS.Client;

namespace Acme.Api.Channels
{
    // NATS: a subject with wildcards matches the subjects published to, by name.
    public class NatsDelivery
    {
        private readonly IConnection _connection;

        public NatsDelivery(IConnection connection) { _connection = connection; }

        public void Track(byte[] data) { _connection.Publish("delivery.eu.tracked", data); }

        public void Watch()
        {
            _connection.SubscribeAsync("delivery.*.tracked", (sender, args) => { Console.WriteLine(args.Message.Subject); });
        }

        public void Poll()
        {
            IConnection connection = new ConnectionFactory().CreateConnection("nats://nats.example.com");
            connection.SubscribeAsync("delivery.polled");
        }
    }
}
