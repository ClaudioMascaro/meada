import { API_HOST } from "./routes";
import { USERS, USER } from "./index";

const BASE = process.env.API_URL;

// exact: an imported host constant and a path constant imported through a re-export.
export async function listUsers() {
  return fetch(API_HOST + USERS);
}

// inferred: a local base URL constant and an imported constant built from another.
export async function getUser() {
  return fetch(`${BASE}${USER}`);
}

// exact: a constant inside the function, folded from imported ones.
export async function firstPage() {
  const url = `${API_HOST}${USERS}?page=1`;
  return fetch(url, { method: "GET" });
}
