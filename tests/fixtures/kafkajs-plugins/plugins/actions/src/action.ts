import { Kafka, Producer } from 'kafkajs';

// A template action publishing to the topic its input names, known only at run time.
export class PublishAction {
  private kafka: Kafka;
  private producer: Producer;

  constructor(brokers: string[]) {
    this.kafka = new Kafka({ clientId: 'actions', brokers });
    this.producer = this.kafka.producer();
  }

  async handler(ctx: { input: { topic: string } }) {
    await this.producer.send({ topic: ctx.input.topic, messages: [{ value: 'scaffolded' }] });
  }
}
