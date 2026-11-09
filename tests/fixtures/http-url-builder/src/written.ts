const BASE = "https://api.example.com";

// unresolved: a write to the path leaves the URL unknown.
export function vets() {
  const u = new URL(`${BASE}/api/vets`);
  u.pathname = "/api/owners";
  return fetch(u.toString());
}

// unresolved: so does a write to the host.
export function owners() {
  const u = new URL(`${BASE}/api/owners`);
  u.host += ".invalid";
  return fetch(u.href);
}
