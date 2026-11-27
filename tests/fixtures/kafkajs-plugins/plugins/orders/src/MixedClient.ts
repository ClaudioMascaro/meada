import { Kafka, Producer } from 'kafkajs';

// A field one method sets from the factory and another from a parameter: it is no client.
export class MixedClient {
  private kafka: Kafka;
  private producer: Producer | null = null;

  constructor() {
    this.kafka = new Kafka({ clientId: 'mixed', brokers: [] });
  }

  connect() {
    this.producer = this.kafka.producer();
  }

  reuse(other: Producer) {
    this.producer = other;
  }

  async send() {
    if (!this.producer) {
      return;
    }
    await this.producer.send({ topic: 'orders.mixed', messages: [] });
  }
}
