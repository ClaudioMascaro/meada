import amqp from "amqplib";
import { producer } from "./kafka";

const connection = await amqp.connect("amqp://localhost");
const channel = await connection.createChannel();

// A queue named like the Kafka topic below: the two never link.
export async function queueOrder(id: string) {
  channel.sendToQueue("orders", Buffer.from(id));
}

export async function kafkaOrder(id: string) {
  await producer.send({ topic: "orders", messages: [{ value: id }] });
}

// Publishes to an exchange reach the queues the worker binds to it.
export async function announce(id: string) {
  channel.publish("events", "order.created", Buffer.from(id));
  channel.publish("events", "order.shipped", Buffer.from(id));
  channel.publish("events", "order.refunded", Buffer.from(id));
}

// The empty exchange is the queue named by the routing key.
export async function dispatch(id: string) {
  channel.publish("", "jobs", Buffer.from(id));
}
