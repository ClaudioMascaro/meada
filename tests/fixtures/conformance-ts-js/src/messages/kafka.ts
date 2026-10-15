import * as kafkajs from "kafkajs";
import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "conformance", brokers: ["kafka:9092"] });

export const producer = kafka.producer();
export const consumer = kafka.consumer({ groupId: "orders" });
export const retrier = kafka.consumer({ groupId: "retries" });

// A namespace import reaches the same factory.
const other = new kafkajs.Kafka({ clientId: "audit", brokers: ["kafka:9092"] });
export const auditor = other.consumer({ groupId: "audit" });
