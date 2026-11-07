# Console frontend (`ui/`)

The static console (ADR 0014) is a separate Vite/React/strict TypeScript build. `npm ci`,
`npm run wasm`, `npm run build`, `npm run check:version` and `npm test` run inside `ui/`, with
Node pinned by `.nvmrc` and wasm-pack by `scripts/wasm.mjs`. Cargo never invokes them. The
WASM bundle builds from `meada-core` into ignored `ui/wasm/`; Vite puts content-hashed scripts,
styles and WASM under `dist/assets/`, with `meada-ui.json` carrying the workspace version.

The pages, the auth and the build: [map/console.md](../console.md).
