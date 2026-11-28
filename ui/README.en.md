# Web console

React application for workspace administration in the browser.

## Interface

Vite uses the React plugin. The package provides commands for development, tests, builds and version checks. `[vite.config.ts:defineConfig]` `[package.json:scripts]`

## Invariants

Tests use jsdom and up to two workers. The build checks types before bundling the application. `[vite.config.ts:maxWorkers]` `[package.json:typecheck]`

## Side effects

The build generates assets and records the distribution version in `dist/`. `[scripts/version.mjs:output]`

## Components

- [Console build](./scripts/README.en.md)
- [Console application](./src/README.en.md)

## Verification

For a fresh checkout, use the Node version in `.nvmrc` and run from the repository root.

```sh
npm --prefix ui ci
npm --prefix ui run wasm
npm --prefix ui run build
npm --prefix ui run check:version
npm --prefix ui test
```
