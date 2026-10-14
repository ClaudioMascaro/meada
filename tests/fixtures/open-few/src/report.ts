declare function tenant(): string;

// unresolved: the host is only known at run time. The method is GET and the path starts with
// `/api/reports/`, which leaves a few routes of this repository.
export function report(name: string) {
  return fetch(`https://${tenant()}.example.com/api/reports/${name}`);
}
