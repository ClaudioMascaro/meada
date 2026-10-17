// Unresolved: the URL is only known at run time. The decisions file `chain-drift-web` links it
// to the catalog's `GET /api/reviews` (`decided`), where `votes` is missing.
export async function rate(event: { url: string }) {
  const res = await fetch(event.url);
  const body = await res.json();
  return [body.stars, body.votes];
}

// Ambiguous: no host, and both the gateway and the catalog serve `GET /api/ping`. Neither
// returns `uptime`, but an ambiguous link gets no drift.
export async function ping() {
  const res = await fetch("/api/ping");
  const body = await res.json();
  return [body.ok, body.uptime];
}
