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
