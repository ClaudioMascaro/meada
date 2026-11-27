import { Kafka } from 'kafkajs';

const kafka = new Kafka({ brokers: [] });
const producer = kafka.producer();

// A producer whose topic is read from the environment.
export async function publish() {
  await producer.send({ topic: process.env.ORDERS_TOPIC, messages: [] });
}
