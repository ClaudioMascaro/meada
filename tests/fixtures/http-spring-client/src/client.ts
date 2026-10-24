// inferred, any method: `@RequestMapping("/any")` serves DELETE too.
export async function drop() {
  return fetch("/api/orders/any", { method: "DELETE" });
}

// inferred: `@PutMapping` with a `{id}` path, after the class prefix.
export async function replace(id: string) {
  return fetch(`/api/orders/${id}`, { method: "PUT" });
}

// no link: `@PostMapping(path = "/new")` serves POST alone, and the context path is not applied.
export async function create() {
  return fetch("/ctx/api/orders/new", { method: "POST" });
}
