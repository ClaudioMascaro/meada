// One hop for each entry field without `exports`: `types`, `module`, and a subpath read as a
// path with the extension and `index` rules. A field whose file is not a source (`dts`) does not
// stop the search, and a package with no entry field reads as `./index` (`plain`, `bare`).
import { typed } from '@example/typed';
import { esm } from '@example/esm';
import { tool } from '@example/esm/lib/tool';
import { part } from '@example/esm/lib/parts';
import { dts } from '@example/dts';
import { plain } from '@example/plain';
import { thing } from '@example/bare';

export function entries(): void {
  typed();
  esm();
  tool();
  part();
  dts();
  plain();
  thing();
}
