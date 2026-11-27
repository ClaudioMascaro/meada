import { Kafka } from 'kafkajs';

const kafka = new Kafka({ clientId: 'peer-worker', brokers: ['kafka:9092'] });
const consumer = kafka.consumer({ groupId: 'peer-worker' });

export async function consumeEvents() {
  await consumer.subscribe({ topic: 'events.published' });
  await consumer.run({ eachMessage: async () => {} });
}
