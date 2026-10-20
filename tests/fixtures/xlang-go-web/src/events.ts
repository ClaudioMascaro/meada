import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "web", brokers: ["kafka:9092"] });
const producer = kafka.producer();

// The api service (Go) consumes the topic.
export async function announceShipment(id: string) {
  await producer.send({ topic: "orders.shipped", messages: [{ value: id }] });
}
