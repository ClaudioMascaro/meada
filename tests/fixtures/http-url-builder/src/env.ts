const BASE = process.env.API_URL;
const url = (p: string) => `${BASE}/${p}`;

// The helper keeps the base URL read from the environment.
export function vets() {
  return fetch(url("api/vets"));
}

// The same through a constant.
export function owners() {
  const u = url("api/owners");
  return fetch(u);
}
