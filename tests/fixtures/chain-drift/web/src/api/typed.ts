const BASE = process.env.CATALOG_URL;

// A client wrapper that returns the parsed body as the type its caller gives.
export async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  return (await res.json()) as T;
}

// A wrapper whose type parameter types the request, not the result: the type argument its
// callers give adds no fields to the response.
export async function search<Q>(path: string, query: Q): Promise<unknown> {
  const res = await fetch(`${BASE}${path}?q=${JSON.stringify(query)}`);
  return res.json();
}

// A wrapper whose return type is its type parameter, whatever the body it returns.
export async function fetchAs<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  return res.json();
}

interface Envelope<T> {
  data: T;
}

// A wrapper that returns the type parameter inside another type: not the type of the body.
export async function enveloped<T>(path: string): Promise<Envelope<T>> {
  const res = await fetch(`${BASE}${path}`);
  return res.json();
}
