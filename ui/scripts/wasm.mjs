import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Keep this pin in sync with the standalone frontend job and container build.
const version = '0.15.0';
const installed = spawnSync('wasm-pack', ['--version'], { encoding: 'utf8' });
if (installed.status !== 0 || installed.stdout.trim() !== `wasm-pack ${version}`) {
  console.error(`Install wasm-pack ${version} before building the frontend.`);
  process.exit(1);
}
const build = spawnSync('wasm-pack', [
  'build', 'meada-core', '--target', 'web', '--out-dir', '../ui/wasm', '--no-opt',
  '--', '--locked', '--features', 'wasm-bindgen',
], { cwd: fileURLToPath(new URL('../../', import.meta.url)), stdio: 'inherit' });
process.exit(build.status ?? 1);
