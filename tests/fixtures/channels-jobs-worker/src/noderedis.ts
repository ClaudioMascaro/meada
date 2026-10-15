import { createClient } from "redis";

const client = createClient();

export async function readShipments() {
  return client.xReadGroup("billing", "worker-1", { key: "shipments", id: ">" });
}

// Several streams: a list of keys with their ids.
export async function readBoth() {
  return client.xReadGroup("billing", "worker-1", [
    { key: "orders", id: ">" },
    { key: "refunds", id: ">" },
  ]);
}
