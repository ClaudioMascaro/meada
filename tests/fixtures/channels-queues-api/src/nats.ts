import { connect } from "nats";

const nc = await connect({ servers: "nats:4222" });
const REGION = "eu";

export function orderCreated() {
  nc.publish("orders.created");
  nc.publish(`orders.${REGION}.created`);
}

export function invoicePaid() {
  nc.publish("billing.invoice.paid");
}
