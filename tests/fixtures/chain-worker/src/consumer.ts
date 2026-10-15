import { Kafka } from "kafkajs";
import { notify } from "./notify";

const kafka = new Kafka({ clientId: "worker", brokers: ["kafka:9092"] });
const consumer = kafka.consumer({ groupId: "worker" });

export async function start() {
  await consumer.subscribe({ topic: "orders.created" });
  await consumer.run({ eachMessage: onOrderCreated });
}

// exact: handles `orders.created`, which the gateway produces.
export async function onOrderCreated({ message }: { message: { value: string } }) {
  notify(message.value);
}
