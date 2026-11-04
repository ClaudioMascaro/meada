import { URL } from "./builder";

// unresolved: the file imports its own `URL`, which is no runtime global.
export function vets() {
  return fetch(new URL("https://api.example.com/api/vets").toString());
}
