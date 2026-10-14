declare function tenant(): string;

// unresolved: more than 50 routes of this repository start with `/api/v1/`.
export function item(name: string) {
  return fetch(`https://${tenant()}.example.com/api/v1/${name}`);
}
