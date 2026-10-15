import { connect } from "nats";

const nc = await connect({ servers: "nats:4222" });

// `*` is one token: `orders.created` matches, `orders.eu.created` does not.
export function oneToken() {
  nc.subscribe("orders.*");
}

// `>` is the rest: both orders subjects match.
export function theRest() {
  nc.subscribe("orders.>", { callback: onOrder });
}

// A literal subject is an exact link.
export function literal() {
  nc.subscribe("billing.invoice.paid");
}

// A wildcard in the middle matches one token.
export function middle() {
  nc.subscribe("orders.*.created", {
    callback: (_err, msg) => {
      onOrder(msg);
    },
  });
}

function onOrder(msg: unknown) {
  return msg;
}
