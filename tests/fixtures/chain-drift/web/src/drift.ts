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
