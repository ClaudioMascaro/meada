import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "api", brokers: ["kafka:9092"] });

// The clients every file imports: their calls trace back to `new Kafka()`.
export const producer = kafka.producer();
export const consumer = kafka.consumer({ groupId: "api-audit" });
