const BASE = "https://api.example.com";
const url = (p) => `${BASE}/${p}`;

// exact: the same in JavaScript.
export function vets() {
  return fetch(url("api/vets"));
}
