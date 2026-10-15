const GATEWAY = process.env.GATEWAY_URL;
const CATALOG = process.env.CATALOG_URL;

// The Express route: `warehouse` comes from an imported constant spread. `sku` is present with
// open content (read from the request). `discount` is never returned, `error` only by the 404.
export async function item(sku: string) {
  const res = await fetch(`${GATEWAY}/api/inventory/${sku}`);
  const stock = await res.json();
  return [stock.warehouse, stock.sku.length, stock.price.currency, stock.discount, stock.error];
}

// The route saves what it receives: a spread of an unknown value, so nothing is checked.
export async function save() {
  const res = await fetch(`${GATEWAY}/api/inventory`, { method: "POST" });
  const saved = await res.json();
  return saved.anything;
}

// The Next.js route: `name.length` stops at the scalar, `tags` comes from a constant, `history`
// is open, `weight` is missing.
export async function stock() {
  const res = await fetch(`${CATALOG}/api/stock`);
  const body = await res.json();
  return [body.name.length, body.tags[0].label, body.history[0].at, body.weight];
}

export async function restock() {
  const res = await fetch(`${CATALOG}/api/stock`, { method: "POST" });
  return (await res.json()).whatever;
}

// The 404 body is left out of the route's shape: `error` is missing.
export async function one(sku: string) {
  const res = await fetch(`${CATALOG}/api/stock/${sku}`);
  const body = await res.json();
  return [body.count, body.error];
}
