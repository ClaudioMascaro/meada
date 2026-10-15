const BASE = process.env.SERVICE_URL;

// `fetch` received through a parameter: destructured, positional, and with a default
// that is not `fetch`.
export async function pushReport({ body, fetchImpl = fetch } = {}) {
  return fetchImpl(`${BASE}/reports`, { method: "POST", body });
}

export async function listReports(request = fetch) {
  return request(`${BASE}/reports`);
}

// A wrapper whose caller supplies the URL still folds.
export async function getItem(path: string, { fetchImpl = fetch } = {}) {
  return fetchImpl(`${BASE}${path}`);
}

export const getOne = () => getItem("/items/1");

// Not `fetch`: no consumer.
export async function other({ client = axios }) {
  return client(`${BASE}/other`);
}

export async function bare(send) {
  return send(`${BASE}/bare`);
}

// Hidden names: a binding spelled like a constant or a parameter of the same function hides
// it, so neither call is read through it.
export async function mirrors(urls: string[]) {
  const url = `${BASE}/health`;
  for (const url of urls) fetch(url);
}

export async function retry(request = fetch) {
  try {
    throw new Error("x");
  } catch (request) {
    return request(`${BASE}/again`);
  }
}
