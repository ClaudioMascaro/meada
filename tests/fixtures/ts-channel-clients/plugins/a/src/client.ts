export class KafkaClient {
  sendMessage(topic: string, message: string) {}
  createConsumer(topic: string) {}
}

export function publish() {
  const client = new KafkaClient();
  client.sendMessage(process.env.ORDERS_TOPIC, "message");
}

export function subscribe(client: KafkaClient) {
  client.createConsumer(process.env.ORDERS_TOPIC);
}

export class Service {
  constructor(private client: KafkaClient) {}
  listen() { this.client.createConsumer(process.env.ORDERS_TOPIC); }
}

export function listenTopic(client: KafkaClient, topic: string) {
  client.createConsumer(topic);
}

export function listenOrders(client: KafkaClient) {
  listenTopic(client, 'orders.created');
}
