const CATALOG = process.env.CATALOG_URL;

// `labels.title` and `labels.other` are under the open `labels`: not checked. `status` is
// not returned: drift.
export async function labels() {
  const res = await fetch(`${CATALOG}/api/labels`);
  const body = await res.json();
  return [body.ok, body.labels.title, body.labels.other, body.status];
}
