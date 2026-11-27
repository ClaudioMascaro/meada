import { Kafka, Producer } from 'kafkajs';

// A public field may be set from outside the class: it is no client.
export class PublicClient {
  private kafka: Kafka;
  producer: Producer | null = null;

  constructor() {
    this.kafka = new Kafka({ clientId: 'public', brokers: [] });
  }

  connect() {
    this.producer = this.kafka.producer();
  }

  async send() {
    if (!this.producer) {
      return;
    }
    await this.producer.send({ topic: 'orders.public', messages: [] });
  }
}
