const BASE = process.env.BACKEND_URL;

// A client wrapper method: its path comes from its parameter.
export class Api {
  get(path: string) {
    return fetch(`${BASE}${path}`);
  }

  // An exact caller: `this` is an Api.
  orders() {
    return this.get("/orders");
  }
}

// A top-level wrapper function.
export function fetchJson(path: string) {
  return fetch(`${BASE}${path}`);
}
