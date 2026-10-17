import type { z } from "zod";
import { Quote } from "@chain/contracts";
import type { Receipt, Status } from "@chain/contracts";
import { getJson } from "./api/typed";

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

// An annotation with the shared interface: the route returns no `paidAt`.
export async function annotated() {
  const res = await fetch(`${CATALOG}/api/receipts`);
  const r: Receipt = await res.json();
  return r;
}

// A type argument given to the client wrapper: the route returns no `paidAt`.
export async function argument() {
  return getJson<Receipt>("/api/receipts");
}

// `z.infer` of the shared schema: the receipt has no `sku` or `price`.
export async function inferred() {
  const res = await fetch(`${CATALOG}/api/receipts`);
  const q: z.infer<typeof Quote> = await res.json();
  return q;
}

// The package's default schema: there is no `row`.
export async function ticket() {
  const res = await fetch(`${CATALOG}/api/tickets`);
  const t = await res.json();
  return [t.seat, t.row];
}

// The schema through a namespace import: there is no `discount`.
export async function offer() {
  const res = await fetch(`${CATALOG}/api/offers`);
  const o = await res.json();
  return [o.sku, o.discount];
}

// The route's return type opens `labels`: `labels.size` is not checked, `title` is missing.
export async function tags() {
  const res = await fetch(`${CATALOG}/api/tags`);
  const t = await res.json();
  return [t.name, t.labels.size, t.title];
}
