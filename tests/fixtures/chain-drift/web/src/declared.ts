import { z } from "zod";
import type { Entity } from "./api/types";
import { getJson } from "./api/typed";
import type { Remote } from "some-lib";

const CATALOG = process.env.CATALOG_URL;

// The profile mirrors the response by hand and extends an imported type. `nickname` and
// `address.zip` are not returned: exact drift. `phone` is optional, so it adds none.
interface Profile extends Entity {
  name: string;
  nickname: string;
  address: { city: string; zip: string };
  phone?: string;
}

interface Page {
  items: Profile[];
  total: number;
  cursor?: string;
}

// A cast to an interface.
export async function cast() {
  const res = await fetch(`${CATALOG}/api/profiles`);
  return (await res.json()) as Page;
}

// A type argument given to the client wrapper that returns the parsed body.
interface Counts {
  total: number;
  count: number;
}

export async function counts() {
  return getJson<Counts>("/api/profiles");
}

// An annotation on the variable, with an intersection through an alias and `Array<T>`.
type Ranked = Counts & { items: Array<{ id: string; rank: number }> };

export async function ranked() {
  const res = await fetch(`${CATALOG}/api/profiles`);
  const page: Ranked = await res.json();
  return page;
}

// `z.infer` of a schema written here: `pages` is not returned.
const Pages = z.object({ total: z.number(), pages: z.number() });

export async function pages() {
  const res = await fetch(`${CATALOG}/api/profiles`);
  const page: z.infer<typeof Pages> = await res.json();
  return page;
}

// A type of a package adds no fields, and nor does a union.
type Maybe = { total: number } | { missing: number };

export async function foreign() {
  const res = await fetch(`${CATALOG}/api/profiles`);
  const remote = (await res.json()) as Remote;
  const either = (await res.json()) as Maybe;
  return [remote, either];
}

// A type argument given to the `json` of a client.
export async function typedJson() {
  const res = await fetch(`${CATALOG}/api/profiles`);
  return res.json<Counts>();
}
