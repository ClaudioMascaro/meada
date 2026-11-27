import { Kafka } from 'kafkajs';

const kafka = new Kafka({ clientId: 'peer-worker', brokers: ['kafka:9092'] });
const producer = kafka.producer();

export async function publishCreated() {
  await producer.send({ topic: 'orders.created', messages: [] });
}

export async function publishShipped() {
  await producer.send({ topic: 'orders.shipped', messages: [] });
}
