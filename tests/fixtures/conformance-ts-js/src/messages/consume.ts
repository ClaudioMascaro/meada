import { Kafka } from "kafkajs";
import { onPayment } from "./handlers";
import { auditor, consumer, retrier } from "./kafka";
import { CREATED } from "./topics";

// `topics`, with an `eachMessage` arrow function written in place.
export async function consumeOrders() {
  await consumer.subscribe({ topics: [CREATED, "orders.shipped"] });
  await consumer.run({
    eachMessage: async ({ message }) => {
      record(message);
    },
  });
}

// `topic`, with an `eachBatch` method written in place.
export async function consumeRefunds() {
  await auditor.subscribe({ topic: "refunds" });
  await auditor.run({
    async eachBatch({ batch }) {
      record(batch);
    },
  });
}

// An imported handler, on a consumer made in this function.
export async function consumePayments() {
  const kafka = new Kafka({ clientId: "payments", brokers: ["kafka:9092"] });
  const payments = kafka.consumer({ groupId: "payments" });
  await payments.subscribe({ topic: "payments" });
  await payments.run({ eachMessage: onPayment });
}

// A consumer wrapper with no `run`: each call site subscribes, and the function making it
// handles.
async function listen(topic: string) {
  await retrier.subscribe({ topic });
}

export async function listenRetries() {
  await listen("retries");
}

function record(value: unknown) {
  return value;
}
