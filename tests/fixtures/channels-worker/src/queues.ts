import amqp from "amqplib";

const connection = await amqp.connect("amqp://localhost");
const channel = await connection.createChannel();

// The bindings live here, the publishes in `api`: the broker is shared.
export async function bind() {
  await channel.bindQueue("audit", "events", "order.created");
  await channel.bindQueue("shipments", "events", "*.shipped");
}

export async function work() {
  await channel.consume("orders", onOrder);
  await channel.consume("jobs", onJob);
  await channel.consume("audit", onAudit);
  await channel.consume("shipments", onShipment);
}

// Queues named like the `events` exchange and like an unbound routing key: no publish to the
// exchange reaches them, since no binding routes it there.
export async function lookalikes() {
  await channel.consume("events");
  await channel.consume("order.refunded");
}

function onOrder(message: unknown) {
  return message;
}

function onJob(message: unknown) {
  return message;
}

function onAudit(message: unknown) {
  return message;
}

function onShipment(message: unknown) {
  return message;
}
