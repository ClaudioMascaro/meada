import { KafkaClient } from './client';

export class OrderService {
  constructor(private client: KafkaClient, private topic: string) {}
  publish() { this.client.sendMessage(this.topic, "message"); }
}
