import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "worker", brokers: ["kafka:9092"] });
const consumer = kafka.consumer({ groupId: "worker" });

export async function start() {
  await consumer.subscribe({ topic: "orders.created", fromBeginning: true });
  await consumer.run({ eachMessage: handleOrder });
}

export async function handleOrder({ message }: { message: { value: string } }) {
  return message.value;
}
