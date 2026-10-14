const BASE = process.env.GATEWAY_URL;

// The client wrapper around fetch: its URL and method come from its parameters.
export function request(path: string, method = "GET") {
  return fetch(`${BASE}${path}`, { method });
}
