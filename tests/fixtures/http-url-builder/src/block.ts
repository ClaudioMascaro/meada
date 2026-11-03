const BASE = "https://api.example.com";

// exact: an arrow function whose block body is one `return`.
const url = (p: string) => {
  return `${BASE}/${p}`;
};

// not folded: an arrow function whose block body has two statements.
const trimmed = (p: string) => {
  const path = p.trim();
  return `${BASE}/${path}`;
};

export function vets() {
  return fetch(url("api/vets"));
}

export function owners() {
  return fetch(trimmed("api/owners"));
}
