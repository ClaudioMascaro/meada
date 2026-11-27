import { KafkaClient } from '../../orders/src/client';
export function subscribe() {
  const client = new KafkaClient();
  client.createConsumer('orders.approved');
}
