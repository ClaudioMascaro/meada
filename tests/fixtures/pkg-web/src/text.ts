// Packages that ship build output: the entry points into `dist`, which maps back to source
// through the provider's `tsconfig.json`, or cannot be mapped at all.
import { normalize } from '@example/shared';
import { collapse } from '@example/shared/utils/collapse';
import { seal } from '@example/sealed';

export function clean(raw: string): string {
  return normalize(raw);
}

export function squash(raw: string): string {
  return collapse(raw);
}

export function lock(): string {
  return seal();
}
