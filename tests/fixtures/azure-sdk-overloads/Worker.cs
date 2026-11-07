using Azure.Messaging.ServiceBus;
class Worker {
 async Task InlineOptions() { var bus=new ServiceBusClient(connection); await bus.CreateReceiver("csharp-inline",new ServiceBusReceiverOptions()).ReceiveMessageAsync(); }
 async Task NamedOptions() { var bus=new ServiceBusClient(connection); var options=new ServiceBusReceiverOptions(); await bus.CreateReceiver("csharp-named",options).ReceiveMessageAsync(); }
 async Task NullNamedOptions() { var bus=new ServiceBusClient(connection); await bus.CreateReceiver("csharp-null-named",options:null).ReceiveMessageAsync(); }
 async Task ChangedFields() { var bus=new ServiceBusClient(connection); var options=new ServiceBusReceiverOptions(); options.ReceiveMode=runtime; await bus.CreateReceiver("csharp-fields",options).ReceiveMessageAsync(); }
 async Task Reassigned() { var bus=new ServiceBusClient(connection); var options=new ServiceBusReceiverOptions(); options=opaque; await bus.CreateReceiver("unproven-csharp-reassigned",options).ReceiveMessageAsync(); }
 async Task UnknownOptions(ServiceBusReceiverOptions options) { var bus=new ServiceBusClient(connection); await bus.CreateReceiver("unproven-csharp-unknown",options).ReceiveMessageAsync(); }
 async Task StringSubscription() { var bus=new ServiceBusClient(connection); var subscription="audit"; await bus.CreateReceiver("csharp-topic",subscription,new ServiceBusReceiverOptions()).ReceiveMessageAsync(); }
 ServiceBusReceiver Make(ServiceBusReceiverOptions options) { return new ServiceBusClient(connection).CreateReceiver("csharp-factory",options); }
 async Task ExactFactories() { await Make(new ServiceBusReceiverOptions()).ReceiveMessageAsync(); await Make(opaque).ReceiveMessageAsync(); }
 async Task Unaffected() { var bus=new ServiceBusClient(connection); await bus.CreateReceiver("csharp-unaffected").ReceiveMessageAsync(); }
 class Hidden {
  class ServiceBusReceiverOptions {}
  async Task HiddenOptions() { var bus=new ServiceBusClient(connection); await bus.CreateReceiver("unproven-csharp-hidden",new ServiceBusReceiverOptions()).ReceiveMessageAsync(); }
 }
}
