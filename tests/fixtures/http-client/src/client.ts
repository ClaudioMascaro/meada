// ambiguous: the URL names no host, and two repositories serve GET /api/users.
export async function users() {
  return fetch("/api/users");
}

// inferred: the base URL is unknown, and only the backend serves GET /health.
export async function health(config: { origin: string }) {
  return fetch(`${config.origin}/health`);
}

// unresolved: the URL is computed at run time.
export async function dynamic(endpoint: URL) {
  return fetch(endpoint.href);
}
