import * as nats from "nats";
import { connect } from "nats";
import { connect as fakeConnect } from "not-nats";

const nc = await connect({ servers: "nats:4222" });
const other = await nats.connect({ servers: "nats:4222" });
const fake = await fakeConnect({});
const REGION = "eu";

// Literal, template and namespace-imported publishers.
export function publishAll() {
  nc.publish("jobs.created");
  nc.publish(`jobs.${REGION}.created`);
  other.publish("jobs.audit.created");
}

// `*` is one token, `>` is the rest; each match is inferred.
export function subscribeOne() {
  nc.subscribe("jobs.*");
}

// The `callback` option, a function in place and a definition.
export function subscribeRest() {
  nc.subscribe("jobs.>", {
    callback: (_err, message) => {
      record(message);
    },
  });
  nc.subscribe("jobs.audit.>", { callback: onAudit });
}

// A wildcard in the middle, and a literal subject.
export function subscribeMiddle() {
  nc.subscribe("jobs.*.created");
  nc.subscribe("jobs.created");
}

// Not channels: a connection of another package, and `publish` on any object.
const bus = {
  publish(subject: string) {
    return subject;
  },
};

export function notPublished() {
  fake.publish("jobs.created");
  bus.publish("jobs.created");
}

function onAudit(_err: unknown, message: unknown) {
  return message;
}

function record(value: unknown) {
  return value;
}
