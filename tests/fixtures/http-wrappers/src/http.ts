const BASE = process.env.BACKEND_URL;

// The client wrapper around fetch: its URL and method come from its parameters.
export function request(path: string, method = "GET") {
  return fetch(`${BASE}${path}`, { method });
}

// A second level of wrappers.
export function get(path: string) {
  return request(path);
}

export function remove(path: string) {
  return request(path, "DELETE");
}

// A cycle: retry and backoff call each other before handing over to `get`.
export function retry(path: string, attempts: number): Promise<Response> {
  return attempts > 0 ? backoff(path, attempts) : get(path);
}

function backoff(path: string, attempts: number) {
  return retry(path, attempts - 1);
}
