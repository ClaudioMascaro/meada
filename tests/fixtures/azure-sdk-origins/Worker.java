import com.azure.messaging.servicebus.ServiceBusClientBuilder;
import com.azure.messaging.servicebus.ServiceBusSenderClient;
import com.azure.messaging.servicebus.ServiceBusReceiverClient;
class Worker {
 ServiceBusSenderClient sender(String name) { return new ServiceBusClientBuilder().connectionString(connection).sender().queueName(name).buildClient(); }
 ServiceBusReceiverClient receiver(String topic, String sub) { return new ServiceBusClientBuilder().connectionString(connection).receiver().topicName(topic).subscriptionName(sub).buildClient(); }
 void run() {
  sender("factory-first-java").sendMessage(message);
  sender("factory-second-java").sendMessage(message);
  receiver("topic-first-java", "audit").receiveMessages(1);
  receiver("topic-second-java", "ops").receiveMessages(1);
 }
 void typed(ServiceBusSenderClient client) { client.sendMessage(message); }
 void reassigned() { var client = sender("stale-java"); client = opaque; client.sendMessage(message); }
 ServiceBusSenderClient alternative(boolean flag) { if(flag) return sender("unsafe-java"); return opaque; }
 void opaquePath() { alternative(flag).sendMessage(message); }
 void nameMatched() { foreign.sender("matched-java").sendMessage(message); }
 void unsupported() { sender("unsupported-java").inventedSend(message); }
 ServiceBusSenderClient handleOnly() { return sender("handle-only-java"); }
 void unaffected() { new ServiceBusClientBuilder().connectionString(connection).sender().queueName("unaffected-java").buildClient().sendMessage(message); }
 class Hidden {
  class ServiceBusClientBuilder {}
  void shadowed() { new ServiceBusClientBuilder().connectionString(connection).sender().queueName("hidden-java").buildClient().sendMessage(message); }
 }
}
