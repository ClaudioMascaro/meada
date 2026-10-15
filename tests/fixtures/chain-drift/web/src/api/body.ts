const BASE = process.env.GATEWAY_URL;

// A client wrapper that returns the parsed body: each call site reads the response value.
export async function get(path: string) {
  const res = await fetch(`${BASE}${path}`);
  if (!res.ok) {
    throw new Error(res.statusText);
  }
  return res.json();
}
