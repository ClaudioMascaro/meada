// inferred, any method: a Django view without restrictions serves POST /health too.
export async function ping() {
  return fetch("/health", { method: "POST" });
}

// inferred: `require_http_methods` lets DELETE through, not as "any method".
export async function cancel(id: string) {
  return fetch(`/shop/orders/${id}`, { method: "DELETE" });
}

// no link: the view restricts the route to GET and HEAD.
export async function restock(sku: string) {
  return fetch(`/shop/items/${sku}/stock`, { method: "PUT" });
}

// unresolved: the path is unknown; the routes serving PUT, every method among them, fit.
export async function archived(path: string) {
  return fetch(path, { method: "PUT" });
}
