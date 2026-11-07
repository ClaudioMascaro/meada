import com.azure.messaging.servicebus.ServiceBusClientBuilder;
import com.azure.messaging.eventhubs.EventHubClientBuilder;
import com.azure.storage.blob.BlobServiceClientBuilder;
import com.azure.security.keyvault.secrets.SecretClientBuilder;
class Worker {
 void send() { new ServiceBusClientBuilder().connectionString(connection).sender().queueName("orders-java").buildClient().sendMessage(message); }
 void receive() { new ServiceBusClientBuilder().connectionString(connection).receiver().queueName("orders-java").buildClient().receiveMessages(1); }
 void subscribe() { new ServiceBusClientBuilder().connectionString(connection).receiver().topicName("orders-java").subscriptionName("audit").buildClient().receiveMessages(1); }
 void sendEvents() { new EventHubClientBuilder().connectionString(connection,"events-java").buildProducerClient().send(batch); }
 void receiveEvents() { new EventHubClientBuilder().connectionString(connection,"events-java").consumerGroup("group").buildConsumerClient().receiveFromPartition("0",1,position); }
 void write() { new BlobServiceClientBuilder().endpoint(endpoint).credential(credential).buildClient().getBlobContainerClient("uploads-java").getBlobClient(object).upload(data); }
 void read() { new BlobServiceClientBuilder().endpoint(endpoint).buildClient().getBlobContainerClient("uploads-java").getBlobClient(object).downloadContent(); }
 void secret() { new SecretClientBuilder().vaultUrl(vault).credential(credential).buildClient().getSecret("token-java"); }
}
