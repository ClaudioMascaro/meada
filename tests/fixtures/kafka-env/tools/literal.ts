import { Kafka } from 'kafkajs';

const kafka = new Kafka({ brokers: [] });
const producer = kafka.producer();
const consumer = kafka.consumer({ groupId: 'tools' });

// The literal counterparts, outside every workload.
export async function replay() {
  await producer.send({ topic: 'orders.created', messages: [] });
}

export async function audit() {
  await consumer.subscribe({ topic: 'orders.created' });
}
