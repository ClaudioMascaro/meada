import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "worker-all", brokers: ["kafka:9092"] });
const consumer = kafka.consumer({ groupId: "worker-all" });

// A regular expression names no topic: this side is never linked.
export async function watchOrders() {
  await consumer.subscribe({ topic: /^orders\./ });
  await consumer.run({ eachMessage: seen });
}

export async function seen({ message }: { message: { value: string } }) {
  return message.value;
}
