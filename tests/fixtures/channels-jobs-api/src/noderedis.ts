import { createClient } from "redis";

const client = createClient();

export async function appendShipment(id: string) {
  await client.xAdd("shipments", "*", { id });
}
