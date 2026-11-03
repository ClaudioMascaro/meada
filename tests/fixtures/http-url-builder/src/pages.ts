import { ownersUrl, url } from "./builder";

// exact: the builder is imported and called in the argument.
export function vets() {
  return fetch(url("api/vets"));
}

// exact: a constant holds the built URL.
export function owners() {
  const u = url("api/owners");
  return fetch(u);
}

// exact: a function with a lone `return`.
export function ownersAgain() {
  return fetch(ownersUrl("api/owners"));
}
