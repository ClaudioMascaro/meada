const BASE = "https://api.example.com";

// exact: `new URL(x)` with one argument parses `x`, and `toString()` is its `href`.
export function vets() {
  return fetch(new URL(`${BASE}/api/vets`).toString());
}

// exact: a constant holds the URL, and `href` reads it.
export function owners() {
  const u = new URL(`${BASE}/api/owners`);
  return fetch(u.href);
}

// exact: `String(u)` and a template string pass the URL through.
export function vetsAsString() {
  const u = new URL(`${BASE}/api/vets`);
  return fetch(String(u));
}

export function ownersInTemplate() {
  const u = new URL(`${BASE}/api/owners`);
  return fetch(`${u}`);
}

// exact: `searchParams` changes only the query.
export function vetsQuery() {
  const u = new URL(`${BASE}/api/vets`);
  u.searchParams.set("page", "1");
  u.search += "&size=10";
  u.hash = "top";
  return fetch(u.toString());
}

// exact: a client wrapper folds at its call sites.
function get(url: string) {
  return fetch(url);
}

export function ownersThroughWrapper() {
  return get(new URL(`${BASE}/api/owners`).toString());
}
