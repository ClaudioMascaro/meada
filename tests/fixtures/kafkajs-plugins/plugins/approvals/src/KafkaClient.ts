import { Config } from '@backstage/config';
import { Consumer, Kafka } from 'kafkajs';

// A client whose consumer is an ECMAScript private field.
export class KafkaClient {
  private kafka: Kafka;
  #consumer: Consumer | null = null;

  constructor(brokers: string[]) {
    this.kafka = new Kafka({ clientId: 'approvals', brokers });
  }

  async connect(groupId: string) {
    this.#consumer = this.kafka.consumer({ groupId });
  }

  async listenApproved(config: Config) {
    await this.connect('approvals');
    await this.subscribe(config.getOptionalString('acme.orders.topics.approved'), async () => {});
  }

  async listenShipped(config: Config) {
    await this.connect('approvals');
    await this.subscribe(config.getString('acme.approvals.topics.shipped'), async () => {});
  }

  async subscribe(topic: string | undefined, handler: (value: string) => Promise<void>) {
    if (!this.#consumer) {
      return;
    }
    await this.#consumer.subscribe({ topic });
    await this.#consumer.run({ eachMessage: async ({ message }) => handler(String(message.value)) });
  }
}
