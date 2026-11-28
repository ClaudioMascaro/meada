# Console build

Build helpers for the WASM core and web package version.

## Interface

`wasm.mjs` builds the browser library. `version.mjs --check` compares the bundle version to the Cargo workspace version. `[wasm.mjs:build]` `[version.mjs:--check]`

## Invariants

The WASM step requires the specified wasm-pack version and uses the Cargo lockfile. `[wasm.mjs:version]` `[wasm.mjs:--locked]`

## Side effects

The scripts invoke wasm-pack, write to `ui/wasm` and write `dist/meada-ui.json` when recording the version. `[wasm.mjs:spawnSync]` `[version.mjs:writeFileSync]`

## Verification

From the repository root, run `npm --prefix ui run wasm && npm --prefix ui run build && npm --prefix ui run check:version`.
