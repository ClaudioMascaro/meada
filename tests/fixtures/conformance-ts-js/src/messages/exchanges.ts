import { channel } from "./rabbit";

const EVENTS = "events";

// Bindings: a pattern with no wildcard, `#` (zero or more words) and `*` (one word).
export async function bind() {
  await channel.bindQueue("audit", EVENTS, "order.created");
  await channel.bindQueue("alerts", EVENTS, "order.#");
  await channel.bindQueue("shipments", EVENTS, "*.shipped");
}

export async function listen() {
  await channel.consume("audit", onAudit);
  await channel.consume("alerts", onAlert);
  await channel.consume("shipments", onShipment);
}

// Publishes: to the exchange `events`, and to the empty exchange, which is the queue it names.
export function announce() {
  channel.publish(EVENTS, "order.created", Buffer.from("{}"));
  channel.publish(EVENTS, "order.shipped", Buffer.from("{}"));
  channel.publish("", "tasks", Buffer.from("{}"));
  channel.publish(EVENTS, "refund.paid", Buffer.from("{}"));
}

function onAudit(message: unknown) {
  return message;
}

function onAlert(message: unknown) {
  return message;
}

function onShipment(message: unknown) {
  return message;
}
