import kit, { greet, farewell, nothing, Client } from '@example/kit';
import * as all from '@example/kit';
import { format as fixed } from '@example/kit/format';
import { slugify } from '@example/kit/utils/slug';
import { secret } from '@example/kit/internal/secret';
import { greet as legacy } from '@example/kit/src/legacy.ts';
import express from 'express';

export function welcome(name: string): string {
  return greet(name);
}

export function main(): void {
  kit();
  all.farewell('web');
  fixed(1);
  slugify('A B');
  Client.create();
  // A `null` target, a name the package does not export, a subpath `exports` does not
  // list, a package outside the workspace, a name never imported.
  secret();
  nothing();
  legacy();
  express();
  shout();
}

// A local binding of the imported name hides the import.
export function hidden(greet: (s: string) => string): string {
  return greet('param');
}

export function caught(): void {
  try {
    welcome('x');
  } catch (farewell) {
    farewell();
  }
}

export function destructured(o: { greet: () => void }): void {
  const { greet } = o;
  greet();
}

export function shadowed(): void {
  let slugify = (s: string) => s;
  slugify('x');
}
