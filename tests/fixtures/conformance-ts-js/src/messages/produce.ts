import { Kafka as Fake } from "not-kafkajs";
import { producer } from "./kafka";
import { CREATED, ORDERS } from "./topics";

// A literal, an imported constant built with `+`, a template literal.
export async function produceAll() {
  await producer.send({ topic: "payments", messages: [] });
  await producer.send({ topic: CREATED, messages: [] });
  await producer.send({ topic: `${ORDERS}.shipped`, messages: [] });
}

// Client wrappers two levels deep: each call site of `emit` produces.
function deliver(topic: string, value: string) {
  return producer.send({ topic, messages: [{ value }] });
}

export function emit(topic: string) {
  return deliver(topic, "{}");
}

export function emitRefund() {
  return emit("refunds");
}

// A wrapper that calls itself: the cycle adds nothing, its caller produces.
function relay(topic: string, attempts: number): unknown {
  if (attempts > 0) {
    return relay(topic, attempts - 1);
  }
  return producer.send({ topic, messages: [] });
}

export function retry() {
  return relay("retries", 3);
}

// Not producers: `send` on an object, and on a client of another package.
const transport = {
  send(message: { topic: string }) {
    return message;
  },
};
const fake = new Fake({ brokers: [] });
const fakeProducer = fake.producer();

export function notProduced() {
  transport.send({ topic: "payments" });
  fakeProducer.send({ topic: "payments" });
}
