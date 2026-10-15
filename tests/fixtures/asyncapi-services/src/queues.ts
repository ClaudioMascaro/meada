import amqp from "amqplib";

const connection = await amqp.connect("amqp://localhost");
const channel = await connection.createChannel();

// Documented as a `send` operation of `events`: the document is the producer.
export async function work() {
  await channel.consume("invoices.issued", onInvoice);
}

function onInvoice(message: unknown) {
  return message;
}

export async function ship(parcel: string) {
  channel.sendToQueue("shipments.requested", Buffer.from(parcel));
}

export async function cancel(parcel: string) {
  channel.sendToQueue("shipments.cancelled", Buffer.from(parcel));
}
