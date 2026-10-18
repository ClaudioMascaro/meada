const CATALOG = process.env.CATALOG_URL;

declare function show(value: unknown): void;
declare function other(): any;

// Nested destructuring with a rename and a default: `page.size` is missing.
export async function pageOf() {
  const res = await fetch(`${CATALOG}/api/batches`);
  const { items: rows = [], page: { number, size } } = await res.json();
  return [rows.length, number, size];
}

// `for...of` over `items` reads `items[].id` (present) and `items[].weight` (missing); the
// nested destructuring of the element reads `owner.name` (present) and `owner.email` (missing).
export async function ids() {
  const res = await fetch(`${CATALOG}/api/batches`);
  const body = await res.json();
  const out = [];
  for (const item of body.items) {
    out.push(item.id, item.weight);
  }
  for (const { owner: { name, email } } of body.items) {
    out.push(name, email);
  }
  return out;
}

// The element parameter of array methods: `qty` is present, `sku` and `note` are missing.
export async function quantities() {
  const res = await fetch(`${CATALOG}/api/batches`);
  const body = await res.json();
  const some = body.items.filter((i: any) => i.sku);
  body.items.forEach(({ note }: any) => show(note));
  return body.items.map((i: any) => i.qty).concat(some);
}

// A destructured name that a `catch` binds too is not the response: nothing is read below it.
export async function rebound() {
  const res = await fetch(`${CATALOG}/api/batches`);
  const { items } = await res.json();
  try {
    return items.map((i: any) => i.gone);
  } catch (items) {
    return (items as any).message;
  }
}

// A loop variable assigned again and a callback parameter shadowed by an inner one are not the
// response: only the elements of `items` are read.
export async function shadowed() {
  const res = await fetch(`${CATALOG}/api/batches`);
  const { items } = await res.json();
  for (let item of items) {
    item = other();
    show(item.gone);
  }
  return items.map((x: any) => {
    const inner = (x: any) => x.nope;
    return [inner(x), x.id];
  });
}

// Values handed to another function are not followed: nothing below them is read.
export async function handed() {
  const res = await fetch(`${CATALOG}/api/batches`);
  const { items, page } = await res.json();
  const first = items[0];
  show(first);
  show(page);
  items.forEach(show);
  return items.map((i: any) => show(i));
}
