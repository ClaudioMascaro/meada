const BASE = process.env.CATALOG_URL;

// A client wrapper that returns the parsed body as the type its caller gives.
export async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  return (await res.json()) as T;
}
