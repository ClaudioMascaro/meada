const CATALOG = process.env.CATALOG_URL;

// `rating` is no longer in the schema: missing. `meta` is a record, so what is read below it is
// not checked. `title.length` stops at the scalar.
export async function comments() {
  const res = await fetch(`${CATALOG}/api/comments`);
  const list = await res.json();
  return [list[0].id, list[0].title.length, list[0].rating, list[0].meta.stars];
}

export async function submit() {
  const res = await fetch(`${CATALOG}/api/comments`, { method: "POST" });
  const saved = await res.json();
  return [saved.id, saved.author?.name, saved.rating];
}

// The summary has no `total`.
export async function summary() {
  const res = await fetch(`${CATALOG}/api/comments/summary`);
  const body = await res.json();
  return [body.count, body.total];
}
