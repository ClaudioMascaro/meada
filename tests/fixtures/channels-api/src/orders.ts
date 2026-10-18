import { producer } from "./kafka";
import { BILLING, ORDERS_CREATED } from "./topics";

// Two consumers in two repositories: fan-out, not ambiguity.
export async function createOrder(id: string) {
  await producer.send({ topic: ORDERS_CREATED, messages: [{ key: id, value: id }] });
}

// A template literal over an imported constant.
export async function invoice(id: string) {
  await producer.send({ topic: `${BILLING}.invoices`, messages: [{ value: id }] });
}

// No consumer anywhere: the producer stays, with no link.
export async function ship(id: string) {
  await producer.send({ topic: "orders.shipped", messages: [{ value: id }] });
}

// Consumed in this repository: a real asynchronous hop all the same.
export async function audit(entry: string) {
  await producer.send({ topic: "audit.log", messages: [{ value: entry }] });
}

// A `send` method on an object that is no Kafka producer.
const mailer = {
  send(message: { topic: string }) {
    return message;
  },
};

export function mail() {
  return mailer.send({ topic: ORDERS_CREATED });
}
