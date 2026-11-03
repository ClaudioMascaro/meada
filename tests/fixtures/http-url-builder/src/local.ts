const BASE = "https://api.example.com";
const url = (p: string) => `${BASE}/${p}`;

// exact: the builder is in the same file.
export function vets() {
  return fetch(url("api/vets"));
}
