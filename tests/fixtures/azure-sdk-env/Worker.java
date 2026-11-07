import com.azure.messaging.servicebus.ServiceBusClientBuilder;
class EnvWorker {
 void receive() { new ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName(System.getenv("SUB")).buildClient().receiveMessages(1); }
 void queue() { new ServiceBusClientBuilder().connectionString(connection).receiver().queueName(System.getenv("QUEUE")).buildClient().receiveMessages(1); }
 void literalTopic() { new ServiceBusClientBuilder().connectionString(connection).receiver().topicName("literal").subscriptionName(System.getenv("SUB")).buildClient().receiveMessages(1); }
 void literalSub() { new ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName("fixed").buildClient().receiveMessages(1); }
 void unread() { new ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName(System.getenv("UNREAD")).buildClient().receiveMessages(1); }
 void literalQueue() { new ServiceBusClientBuilder().connectionString(connection).receiver().queueName("unaffected-java").buildClient().receiveMessages(1); }
}
