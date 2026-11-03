const BASE = "https://api.example.com";

// A `url` the call does not reach: a method of an object of unknown class.
export function vets(client: { url(p: string): string }) {
  const url = (p: string) => `${BASE}/${p}`;
  return fetch(client.url("api/vets"));
}
