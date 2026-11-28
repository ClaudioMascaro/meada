# Console web

Aplicação React que administra o workspace no navegador.

## Interface

O Vite usa o plugin React. O pacote oferece comandos para desenvolvimento, testes, build e validação da versão. `[vite.config.ts:defineConfig]` `[package.json:scripts]`

## Invariantes

Os testes usam jsdom e até dois workers. O build verifica os tipos antes de empacotar a aplicação. `[vite.config.ts:maxWorkers]` `[package.json:typecheck]`

## Efeitos colaterais

O build gera os assets e grava a versão da distribuição no diretório `dist/`. `[scripts/version.mjs:output]`

## Componentes

- [Build do console](./scripts/README.md)
- [Aplicação do console](./src/README.md)

## Verificação

Para um checkout novo, use a versão de Node em `.nvmrc` e execute na raiz.

```sh
npm --prefix ui ci
npm --prefix ui run wasm
npm --prefix ui run build
npm --prefix ui run check:version
npm --prefix ui test
```
