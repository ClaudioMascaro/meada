import { get, remove, retry } from "./http";

// inferred, two wrapper levels down: get, then request.
export function listUsers() {
  return get("/api/users");
}

// inferred: nobody passes `id` in, so it stays a path parameter.
export function deleteUser(id: string) {
  return remove(`/api/users/${id}`);
}

// inferred, with type arguments under `await`.
export async function listUsersTyped() {
  const users = await get<string[]>("/api/users");
  return users;
}

// inferred, through the cycle: retry reaches get without looping.
export function listUsersPatiently() {
  return retry("/api/users", 3);
}
