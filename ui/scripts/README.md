# Build do console

Preparação do núcleo WASM e da versão do pacote web.

## Interface

`wasm.mjs` compila a biblioteca para o navegador. `version.mjs --check` compara a versão do bundle com a do workspace Cargo. `[wasm.mjs:build]` `[version.mjs:--check]`

## Invariantes

A etapa WASM exige a versão especificada de wasm-pack e usa o lockfile do Cargo. `[wasm.mjs:version]` `[wasm.mjs:--locked]`

## Efeitos colaterais

Os scripts invocam wasm-pack, gravam em `ui/wasm` e escrevem `dist/meada-ui.json` ao registrar a versão. `[wasm.mjs:spawnSync]` `[version.mjs:writeFileSync]`

## Verificação

Na raiz do repositório, execute `npm --prefix ui run wasm && npm --prefix ui run build && npm --prefix ui run check:version`.
