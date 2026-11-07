using System;
using Azure.Messaging.ServiceBus;
class EnvWorker {
 async Task Receive() { await new ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), Environment.GetEnvironmentVariable("SUB")).ReceiveMessagesAsync(1); }
 async Task Queue() { await new ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("QUEUE")).ReceiveMessagesAsync(1); }
 async Task LiteralTopic() { await new ServiceBusClient(connection).CreateReceiver("literal", Environment.GetEnvironmentVariable("SUB")).ReceiveMessagesAsync(1); }
 async Task LiteralSub() { await new ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), "fixed").ReceiveMessagesAsync(1); }
 async Task Unread() { await new ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), Environment.GetEnvironmentVariable("UNREAD")).ReceiveMessagesAsync(1); }
 async Task LiteralQueue() { await new ServiceBusClient(connection).CreateReceiver("unaffected-csharp").ReceiveMessageAsync(); }
}
