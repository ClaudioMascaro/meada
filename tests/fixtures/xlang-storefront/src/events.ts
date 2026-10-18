import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "storefront", brokers: ["kafka:9092"] });
const producer = kafka.producer();

// The inventory service's worker (Python) consumes the topic.
export async function announceRestock(sku: string) {
  await producer.send({ topic: "stock.restocked", messages: [{ value: sku }] });
}
