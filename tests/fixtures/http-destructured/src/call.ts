const BASE = process.env.SERVICE_URL;

interface Request {
  url: string;
  method?: string;
}

// Wrappers whose URL and method come from one destructured object parameter.
export function call({ url, method = "GET" }: Request) {
  return fetch(url, { method });
}

// A renamed property, and a renamed one with a default.
export function send({ url: target, method: verb = "PUT" }: Request) {
  return fetch(target, { method: verb });
}

// Reached through a second wrapper level, and through an options bag left out of the call.
export function post(path: string) {
  return call({ url: `${BASE}${path}`, method: "POST" });
}

export function fetchAll({ url } = { url: `${BASE}/all` }) {
  return call({ url });
}

export const REPORTS = "/reports";

export const listReports = () => call({ url: `${BASE}${REPORTS}` });

export const putReport = () => send({ url: `${BASE}${REPORTS}` });

export const createReport = () => post(REPORTS);

export const listAll = () => fetchAll();

// Not a literal object: the call stays unresolved.
export function forward(options: Request) {
  return call(options);
}
