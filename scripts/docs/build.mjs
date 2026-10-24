// Local static bundle only. No browser process, CDN or runtime API is required.
import { build } from 'esbuild';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const output = resolve(process.argv[2]);
const result = await build({
  entryPoints: [new URL('./flow.jsx', import.meta.url).pathname],
  bundle: true, minify: true, format: 'esm', target: ['es2022'],
  outfile: resolve(output, 'flow.js'), legalComments: 'linked', metafile: true,
  define: { 'process.env.NODE_ENV': '"production"' }
});
// Include complete notices for every package whose code reaches the reader bundle.
const packages = new Set();
for (const input of Object.keys(result.metafile.inputs)) {
  const match = input.match(/node_modules\/((?:@[^/]+\/)?[^/]+)\//);
  if (match) packages.add(match[1]);
}
const notices = [];
for (const name of [...packages].sort()) {
  const root = new URL(`./node_modules/${name}/`, import.meta.url);
  const meta = JSON.parse(await readFile(new URL('package.json', root), 'utf8'));
  let license;
  for (const file of ['LICENSE', 'LICENSE.md', 'LICENSE.txt', 'license', 'license.md']) {
    try { license = await readFile(new URL(file, root), 'utf8'); break; } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
  }
  if (!license) throw new Error(`Missing license notice for bundled dependency ${name}`);
  notices.push(`${name} ${meta.version}\n${'='.repeat(72)}\n${license.trim()}\n`);
}
await writeFile(resolve(output, 'THIRD_PARTY_NOTICES.txt'), notices.join('\n'));
console.log(`Built local graph assets and ${packages.size} dependency notices`);
