import { get } from "./api/body";

const BASE = process.env.GATEWAY_URL;

// Drift: the gateway's document for GET /api/orders returns no `total`.
export async function orderTotal() {
  const res = await fetch(`${BASE}/api/orders`);
  const orders = await res.json();
  return orders[0].total;
}

// No drift: `id` is documented. The read stops at the method call.
export async function firstOrderId() {
  const res = await fetch(`${BASE}/api/orders`);
  const orders = await res.json();
  return orders[0]?.id.toUpperCase();
}

// Not checked: `product` is open in the document. The body is awaited later.
export async function productName() {
  const res = await fetch(`${BASE}/api/orders`);
  const pending = res.json();
  const orders = await pending;
  return orders[0].product.name;
}

// Drift through the client wrapper that returns the parsed body: no `customer`.
export async function customerEmail() {
  const orders = await get("/api/orders");
  return orders[0].customer.email;
}

// A name rebound in the function is not the response value: nothing is read.
export async function rebound() {
  const orders = await get("/api/orders");
  try {
    return orders[0].id;
  } catch (orders) {
    return orders.message;
  }
}

// No drift: `id` is a string in the document, and a read below a scalar stops there.
export async function idLength() {
  const orders = await get("/api/orders");
  return orders[0].id.length;
}

// One wrapper fetching two routes: the two consumers at its caller's call site each read only
// what their own fetch returns (the orders, read by the caller; `carrier`, read here).
async function load(orders: string, shipment: string) {
  const list = await fetch(`${BASE}${orders}`);
  const one = await fetch(`${BASE}${shipment}`);
  console.log((await one.json()).carrier);
  return list.json();
}

export async function page() {
  const orders = await load("/api/orders", "/api/shipments/1");
  return orders[0].id;
}

// Two calls of the wrapper in one wrapper reach the same consumer site: its shape holds the
// reads of both (`total` here, `customer` at the caller).
async function both(path: string) {
  const first = await get(path);
  const second = await get(path);
  console.log(second[0].total);
  return first;
}

export async function pair() {
  const orders = await both("/api/orders");
  return orders[0].customer;
}
