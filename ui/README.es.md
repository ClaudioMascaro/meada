# Consola web

Aplicación React para administrar el workspace desde el navegador.

## Interfaz

Vite utiliza el plugin de React. El paquete ofrece comandos de desarrollo, prueba, compilación y verificación de versión. `[vite.config.ts:defineConfig]` `[package.json:scripts]`

## Invariantes

Las pruebas usan jsdom y hasta dos workers. La compilación verifica los tipos antes de empaquetar la aplicación. `[vite.config.ts:maxWorkers]` `[package.json:typecheck]`

## Efectos secundarios

La compilación genera los recursos y guarda la versión de la distribución en `dist/`. `[scripts/version.mjs:output]`

## Componentes

- [Compilación de la consola](./scripts/README.es.md)
- [Aplicación de la consola](./src/README.es.md)

## Verificación

En un checkout nuevo, usa la versión de Node de `.nvmrc` y ejecuta desde la raíz.

```sh
npm --prefix ui ci
npm --prefix ui run wasm
npm --prefix ui run build
npm --prefix ui run check:version
npm --prefix ui test
```
