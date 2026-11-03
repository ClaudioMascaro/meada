const BASE = process.env.API_URL;
const url = (p: string) => `${BASE}/${p}`;

// The base is read from the environment: no host, so no link.
export function vets() {
  return fetch(url("api/vets"));
}

// The same through a constant.
export function owners() {
  const u = url("api/owners");
  return fetch(u);
}
