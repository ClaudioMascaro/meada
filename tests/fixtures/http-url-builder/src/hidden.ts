import { url } from "./builder";

// The parameter hides the imported builder: it is not a function of the repository.
export function vets(url: (p: string) => string) {
  return fetch(url("api/vets"));
}

// So does a `catch` binding and a local constant.
export function owners(fail: () => void) {
  try {
    fail();
  } catch (url) {
    return fetch(url("api/owners"));
  }
}

export function pets() {
  const url = (p: string) => p.trim();
  return fetch(url("api/pets"));
}

export const unused = url;
