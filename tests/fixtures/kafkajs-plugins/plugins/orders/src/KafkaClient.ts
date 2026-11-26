import { Config } from '@backstage/config';
import { Consumer, Kafka, Producer } from 'kafkajs';

// A client whose consumer is set after construction, in `connect`.
export class KafkaClient {
  private kafka: Kafka;
  private producer: Producer;
  private consumer: Consumer | null = null;

  constructor(brokers: string[]) {
    this.kafka = new Kafka({ clientId: 'orders', brokers });
    this.producer = this.kafka.producer();
  }

  async connect(groupId: string) {
    this.consumer = this.kafka.consumer({ groupId });
    await this.producer.connect();
  }

  async disconnect() {
    this.consumer = null;
  }

  async publishApproved(config: Config) {
    await this.send(config.getString('acme.orders.topics.approved'), 'approved');
  }

  async send(topic: string, message: string) {
    await this.producer.send({ topic, messages: [{ value: message }] });
  }

  async subscribe(topic: string | undefined, handler: (value: string) => Promise<void>) {
    if (!this.consumer) {
      return;
    }
    await this.consumer.subscribe({ topic });
    await this.consumer.run({ eachMessage: async ({ message }) => handler(String(message.value)) });
  }
}
