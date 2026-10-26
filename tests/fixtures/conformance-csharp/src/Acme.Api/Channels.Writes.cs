using System;
using System.Threading.Tasks;
using Azure.Messaging.ServiceBus;
using RabbitMQ.Client;
using RabbitMQ.Client.Events;

namespace Acme.Api.Channels
{
    // A sender written more than once has no one queue: a second constructor hands one in, a
    // setter replaces it, a field initializer is written again, and a local is made twice.
    public class BusRewritten
    {
        private static ServiceBusClient Shared;
        private readonly ServiceBusClient _client;
        private ServiceBusSender _constructed;
        private ServiceBusSender _swapped;
        private ServiceBusSender _replaced = Shared.CreateSender("replaced.first");

        public BusRewritten(ServiceBusClient client)
        {
            _client = client;
            _constructed = client.CreateSender("constructed");
            _swapped = client.CreateSender("swapped");
        }

        internal BusRewritten(ServiceBusSender given) { _constructed = given; }

        public void Use(ServiceBusSender other) => _swapped = other;

        public void Reset() { _replaced = Shared.CreateSender("replaced.second"); }

        public async Task Constructed(ServiceBusMessage message) { await _constructed.SendMessageAsync(message); }

        public async Task Swapped(ServiceBusMessage message) { await _swapped.SendMessageAsync(message); }

        public async Task Replaced(ServiceBusMessage message) { await _replaced.SendMessageAsync(message); }

        public async Task Twice(ServiceBusMessage message)
        {
            ServiceBusSender outbox = _client.CreateSender("twice.first");
            await outbox.SendMessageAsync(message);
            outbox = _client.CreateSender("twice.second");
            await outbox.SendMessageAsync(message);
        }
    }

    // A sender made for the queue a method is given is named by each caller of the method.
    public class BusWrapped
    {
        private readonly ServiceBusClient _client;

        public BusWrapped(ServiceBusClient client) { _client = client; }

        public async Task Send(string queue, ServiceBusMessage message)
        {
            ServiceBusSender wrapped = _client.CreateSender(queue);
            await wrapped.SendMessageAsync(message);
        }

        public async Task Order(ServiceBusMessage message) { await Send("wrapped.orders", message); }

        public async Task Audit(ServiceBusMessage message) { await Send("wrapped.audit", message); }

        public async Task Work()
        {
            var processor = _client.CreateProcessor("wrapped.orders");
            processor.ProcessMessageAsync += OnOrder;
            await processor.StartProcessingAsync();
        }

        private Task OnOrder(ProcessMessageEventArgs args) => Task.CompletedTask;
    }

    // `String.Empty` is the default exchange, a client awaited without capturing the context is
    // still the client, and two handlers on one event are two consumers of the queue.
    public class RabbitFanOut
    {
        private readonly ConnectionFactory _factory;
        private AsyncEventingBasicConsumer _consumer;

        public RabbitFanOut(ConnectionFactory factory) { _factory = factory; }

        public async Task Announce()
        {
            var connection = await _factory.CreateConnectionAsync().ConfigureAwait(false);
            var channel = await connection.CreateChannelAsync().ConfigureAwait(false);
            await channel.BasicPublishAsync(String.Empty, "fanout", false, null, null).ConfigureAwait(false);
        }

        public async Task Attend(IChannel channel)
        {
            _consumer = new AsyncEventingBasicConsumer(channel);
            _consumer.ReceivedAsync += OnFirst;
            _consumer.ReceivedAsync += OnSecond;
            await channel.BasicConsumeAsync(_consumer, "fanout", true);
        }

        private Task OnFirst(object sender, BasicDeliverEventArgs delivery) => Task.CompletedTask;

        private Task OnSecond(object sender, BasicDeliverEventArgs delivery) => Task.CompletedTask;
    }
}
