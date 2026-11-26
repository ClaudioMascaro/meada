export class KafkaClient {
  sendMessage(topic: string, message: string) {}
  createConsumer(topic: string) {}
}

export function publish() {
  const client = new KafkaClient();
  client.sendMessage("orders.shipped", "message");
}
