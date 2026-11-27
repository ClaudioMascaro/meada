export async function globals(k: string, u: string) {
  const m = new Map();
  m.get(k);
  const r = await fetch(u);
  r.json();
  console.info(k);
  JSON.parse(k);
  Math.max(1);
  Object.keys({});
}
export function annotated(r: Response) { r.json(); }
export function inline(k: string) { new Map().get(k); }

export function parsed(value: string) { JSON.parse(value).get("key"); }
export function generic(m: Map<string, string>) { m.get("key"); }
export async function localFetch(fetch: (u: string) => Promise<any>, u: string) {
  const response = await fetch(u);
  response.json();
}

export async function awaited(p: Promise<{ get(k: string): void }>) {
  (await p).get("key");
}
