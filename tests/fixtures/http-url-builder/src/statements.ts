const BASE = "https://api.example.com";

// More than one statement: not folded.
function url(p: string) {
  const path = p.trim();
  return `${BASE}/${path}`;
}

export function vets() {
  return fetch(url("api/vets"));
}
