import { collapse } from './utils/collapse';
import { trim } from './utils/trim';

export function normalize(raw: string): string {
  return collapse(trim(raw)).toLowerCase();
}
