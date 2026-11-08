using System;
using System.Threading.Tasks;
using Amazon.SQS;
using Amazon.SQS.Model;
using Azure.Messaging.ServiceBus;
using Confluent.Kafka;
using RabbitMQ.Client;
using RabbitMQ.Client.Events;
using StackExchange.Redis;

namespace Acme.Api.Channels
{
    public static class Topics
    {
        public const string Created = "orders.created";
        public const string Paid = "orders.paid";
    }

    // Kafka: `Produce` and `ProduceAsync` name the topic they send to, `Subscribe` the topics a
    // polling consumer reads, and the method that subscribes handles what arrives.
    public class KafkaSales
    {
        private readonly IProducer<string, string> _producer;

        public KafkaSales(IProducer<string, string> producer) { _producer = producer; }

        public void Created() { _producer.Produce(Topics.Created, new Message<string, string>()); }

        public async Task Paid()
        {
            await _producer.ProduceAsync(Topics.Paid, new Message<string, string>());
        }

        public void Dynamic()
        {
            _producer.Produce(Environment.GetEnvironmentVariable("SALES_TOPIC"), new Message<string, string>());
        }
    }

    public class KafkaFeed
    {
        public void OnSale()
        {
            using var consumer = new ConsumerBuilder<string, string>(new ConsumerConfig()).Build();
            consumer.Subscribe(new[] { Topics.Created, Topics.Paid });
        }

        public void OnMissing(IConsumer<string, string> consumer)
        {
            consumer.Subscribe(Environment.GetEnvironmentVariable("MISSING_TOPIC"));
        }
    }

    // RabbitMQ: a publish to the default exchange reaches the queue its routing key names, one to
    // a named exchange the queues bound to it; a handler is the one attached to the consumer.
    public class RabbitInvoicing
    {
        private readonly IChannel _channel;

        public RabbitInvoicing(IChannel channel) { _channel = channel; }

        public async Task Invoice()
        {
            await _channel.BasicPublishAsync(exchange: "invoicing", routingKey: "invoice.created", body: null);
        }

        public async Task Refund()
        {
            await _channel.BasicPublishAsync(string.Empty, "refunds", false, null, null);
        }

        public async Task Attend()
        {
            await _channel.QueueBindAsync("invoices", "invoicing", "invoice.*");
            var invoices = new AsyncEventingBasicConsumer(_channel);
            invoices.ReceivedAsync += OnInvoice;
            await _channel.BasicConsumeAsync("invoices", true, invoices);
            var refunds = new AsyncEventingBasicConsumer(_channel);
            refunds.ReceivedAsync += async (model, delivery) => { await Task.Yield(); };
            await _channel.BasicConsumeAsync(queue: "refunds", autoAck: true, consumer: refunds);
        }

        private Task OnInvoice(object sender, BasicDeliverEventArgs delivery) => Task.CompletedTask;
    }

    // SQS: the queue is the last segment of its URL.
    public class SqsShipments
    {
        private const string Url = "https://sqs.us-east-1.amazonaws.com/123456789012/shipments";
        private readonly IAmazonSQS _sqs;

        public SqsShipments(IAmazonSQS sqs) { _sqs = sqs; }

        public async Task Ship() { await _sqs.SendMessageAsync(Url, "body"); }

        public async Task Poll()
        {
            await _sqs.ReceiveMessageAsync(new ReceiveMessageRequest { QueueUrl = Url, MaxNumberOfMessages = 10 });
        }
    }

    // Redis streams: the key names the stream.
    public class RedisAudits
    {
        private readonly IConnectionMultiplexer _redis;

        public RedisAudits(IConnectionMultiplexer redis) { _redis = redis; }

        public async Task Record()
        {
            var db = _redis.GetDatabase();
            await db.StreamAddAsync("audits", "event", "created");
        }

        public async Task Drain()
        {
            var db = _redis.GetDatabase();
            await db.StreamReadGroupAsync("audits", "group", "worker", ">");
        }
    }

    // Service Bus: a sender is named by the queue or topic it is made for, a processor by the
    // queue or the topic (whatever its subscription) it is made for.
    public class BusDispatch
    {
        private readonly ServiceBusClient _client;

        public BusDispatch(ServiceBusClient client) { _client = client; }

        public async Task Send(ServiceBusMessage message)
        {
            ServiceBusSender sender = _client.CreateSender("dispatch");
            await sender.SendMessageAsync(message);
            await _client.CreateSender("dispatch.events").SendMessageAsync(message);
        }

        public async Task Dynamic(ServiceBusMessage message)
        {
            var sender = _client.CreateSender(Environment.GetEnvironmentVariable("DISPATCH_QUEUE"));
            await sender.SendMessageAsync(message);
        }

        public async Task Work()
        {
            var processor = _client.CreateProcessor("dispatch", new ServiceBusProcessorOptions());
            processor.ProcessMessageAsync += OnDispatch;
            await processor.StartProcessingAsync();
        }

        public async Task Watch()
        {
            var audit = _client.CreateProcessor("dispatch.events", "audit");
            audit.ProcessMessageAsync += OnDispatch;
            var mirror = _client.CreateProcessor("dispatch.events", "mirror");
            mirror.ProcessMessageAsync += async args => { await args.CompleteMessageAsync(args.Message); };
            await audit.StartProcessingAsync();
        }

        private Task OnDispatch(ProcessMessageEventArgs args) => Task.CompletedTask;
    }
}
