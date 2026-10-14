// Consumers that stay unlinked, and two that only the unresolved-host rule links.

// Unresolved: the URL is not written at the call.
export async function dynamic(url: string) {
  return fetch(url);
}

// Unresolved: the method is not written at the call.
export async function anyMethod(method: string) {
  return fetch("https://api.example.com/api/users", { method });
}

// No peer serves this host.
export async function logo() {
  return fetch("https://cdn.example.com/assets/logo.png");
}

// inferred: the variable is no peer's `base_url_env`, and one repository serves the route.
export async function other() {
  return fetch(`${process.env.OTHER_URL}/api/users`);
}

// The missing peer's variable: nothing to link to.
export async function invoices() {
  return fetch(`${process.env.BILLING_URL}/invoices`);
}

// inferred: a relative URL names no repository, and one repository serves the route.
export async function local() {
  return fetch("/api/users");
}

// Not a route the api serves.
export async function missingRoute() {
  return fetch("https://api.example.com/api/teams");
}
