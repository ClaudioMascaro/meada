// exact: a literal URL on a host the api serves.
export async function listUsers() {
  return fetch("https://api.example.com/api/users");
}

// exact: host case and a trailing slash do not matter; the method comes from `init`.
export async function removeUser(id: string) {
  return fetch(`https://API.example.com/api/users/${id}/`, { method: "DELETE" });
}

// inferred: the base URL comes from a `base_url_env` variable.
export async function getUser(id: string) {
  return fetch(`${process.env.API_URL}/api/users/${id}?expand=team`);
}

// inferred: `+` concatenation, a lower-case method.
export async function createUser(user: object) {
  return fetch(process.env.NEXT_PUBLIC_API_URL + "/api/users", {
    method: "post",
    body: JSON.stringify(user),
  });
}

// inferred: through a `??` fallback and a string method.
export async function report(path: string) {
  return fetch(`${(process.env.API_URL ?? "http://localhost:3000").replace(/\/$/, "")}/api/reports/${path}#top`);
}

// inferred: `import.meta.env` and `process.env["X"]` reads.
export async function health() {
  await fetch(`${import.meta.env.API_URL}/api/health/`);
  return fetch(process.env["API_URL"].trim() + "/api/health", { method: "GET" });
}
