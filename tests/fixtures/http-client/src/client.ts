// ambiguous: the URL names no host, and two repositories serve GET /api/users.
export async function users() {
  return fetch("/api/users");
}

// inferred: the URL names no host, and only the backend serves GET /health.
export async function health() {
  return fetch("/health");
}

// unresolved: the URL is computed at run time.
export async function dynamic(endpoint: URL) {
  return fetch(endpoint.href);
}

// unresolved: the base URL is unknown and may hold a path of its own, so only the end of the
// path is known: an open item, never a link (#801).
export async function origin(config: { origin: string }) {
  return fetch(`${config.origin}/health`);
}
