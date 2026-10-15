// One hop for each entry field without `exports`: `types`, `module`, and a subpath read as a
// path with the extension and `index` rules.
import { typed } from '@example/typed';
import { esm } from '@example/esm';
import { tool } from '@example/esm/lib/tool';
import { part } from '@example/esm/lib/parts';

export function entries(): void {
  typed();
  esm();
  tool();
  part();
}
