import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';

const manifest = readFileSync(new URL('../../Cargo.toml', import.meta.url), 'utf8');
const workspace = /^\[workspace\.package\]\s*\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m.exec(manifest)?.[1];
const version = /^version\s*=\s*"([^"]+)"/m.exec(workspace ?? '')?.[1];
assert(version, 'Cargo.toml must define workspace.package.version');
const output = new URL('../dist/meada-ui.json', import.meta.url);
if (process.argv.includes('--check')) {
  assert.equal(JSON.parse(readFileSync(output, 'utf8')).version, version);
} else {
  writeFileSync(output, `${JSON.stringify({ version })}\n`);
}
