import type { Receipt, Status } from "@chain/contracts";

const GATEWAY = process.env.GATEWAY_URL;
const CATALOG = process.env.CATALOG_URL;

// The route parses with the shared zod schema: `discount` is not in it.
export async function quote() {
  const res = await fetch(`${CATALOG}/api/quotes`);
  const q = await res.json();
  return [q.sku, q.price, q.discount];
}

// The route spreads the shared constant: `aisle` is not in it.
export async function shelf(id: string) {
  const res = await fetch(`${GATEWAY}/api/shelves/${id}`);
  const s = await res.json();
  return [s.warehouse, s.id, s.aisle];
}

// A cast to the shared interface: the route returns no `paidAt`.
export async function receipt() {
  const res = await fetch(`${CATALOG}/api/receipts`);
  return (await res.json()) as Receipt;
}

// The listing is open: nothing below its root is checked.
export async function shelves() {
  const res = await fetch(`${GATEWAY}/api/shelves`);
  const all = await res.json();
  return [all.count, all.max];
}

// A shared union adds no fields: only `id`, read, is checked.
export async function status() {
  const res = await fetch(`${CATALOG}/api/receipts`);
  const s = (await res.json()) as Status;
  return s.id;
}
