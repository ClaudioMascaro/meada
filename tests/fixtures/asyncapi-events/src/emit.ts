import { Kafka } from "kafkajs";
import amqp from "amqplib";

const kafka = new Kafka({ clientId: "events", brokers: ["kafka:9092"] });
const producer = kafka.producer();

// Documented too in `asyncapi.yaml`: the document adds the payload to this side.
export async function announceUpdate(id: string) {
  await producer.send({ topic: "orders.updated", messages: [{ value: id }] });
}

const connection = await amqp.connect("amqp://localhost");
const channel = await connection.createChannel();

// Documented too in `billing.asyncapi.yaml` (`receive`).
export async function work() {
  await channel.consume("shipments.cancelled", onCancellation);
}

function onCancellation(message: unknown) {
  return message;
}
