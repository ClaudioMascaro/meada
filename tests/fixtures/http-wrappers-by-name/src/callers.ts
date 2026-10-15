import { Api } from "./api";

// Found only by the name `get`: the receiver may be any object, so this is not a consumer.
export function unrelated(store) {
  return store.get("/reports");
}

// Found only by the name `fetchJson`: no import binds it, so it may be another function.
export function bare() {
  return fetchJson("/bare");
}

// A receiver typed only by `new` is still a name match in TypeScript: Meada tells no receiver's
// class but `this`, so this call is not climbed either.
const api = new Api();
export function list() {
  return api.get("/list");
}
