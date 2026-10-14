// ambiguous: no host, and both the gateway and the catalog serve GET /api/health.
export function health() {
  return fetch("/api/health");
}

// unresolved: the URL is only known at run time.
export function track(event: { url: string }) {
  return fetch(event.url, { method: "POST" });
}
