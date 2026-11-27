import { KafkaClient } from './client';
import { OrderService } from './service';

function make(client: KafkaClient, topic: string) {
  return new OrderService(client, topic);
}
export function create(config: { getString(key: string): string }) {
  return make(new KafkaClient(), config.getString('acme.orders.topic'));
}
