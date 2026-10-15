import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "gateway", brokers: ["kafka:9092"] });

export const producer = kafka.producer();
