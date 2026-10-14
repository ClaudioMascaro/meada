// exact: an absolute URL on a host the catalog declares.
export function productOf(id: string) {
  return fetch(`https://catalog.example.com/api/products/${id}`);
}
