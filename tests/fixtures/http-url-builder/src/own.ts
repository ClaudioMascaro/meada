// unresolved: the file declares a `URL` class, whose constructor is the repository's.
class URL {
  constructor(readonly path: string) {}
  toString() {
    return `https://api.example.com/${this.path}`;
  }
}

export function vets() {
  return fetch(new URL("api/vets").toString());
}
