import { Kafka } from "kafkajs";

const kafka = new Kafka({ clientId: "services", brokers: ["kafka:9092"] });
const consumer = kafka.consumer({ groupId: "services" });
const producer = kafka.producer();

export async function listen() {
  await consumer.subscribe({ topic: "orders.created" });
  await consumer.subscribe({ topic: "orders.updated" });
  await consumer.run({ eachMessage: onOrder });
}

export async function onOrder({ message }: { message: { value: string } }) {
  return message.value;
}

// Documented as a `publish` operation of `events`: the document is the consumer.
export async function requestPayment(orderId: string) {
  await producer.send({ topic: "payments.requested", messages: [{ value: orderId }] });
}
