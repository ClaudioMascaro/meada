# Compilación de la consola

Preparación del núcleo WASM y de la versión del paquete web.

## Interfaz

`wasm.mjs` compila la biblioteca para el navegador. `version.mjs --check` compara la versión del bundle con la del workspace de Cargo. `[wasm.mjs:build]` `[version.mjs:--check]`

## Invariantes

La etapa WASM requiere la versión indicada de wasm-pack y utiliza el lockfile de Cargo. `[wasm.mjs:version]` `[wasm.mjs:--locked]`

## Efectos secundarios

Los scripts invocan wasm-pack, escriben en `ui/wasm` y generan `dist/meada-ui.json` al registrar la versión. `[wasm.mjs:spawnSync]` `[version.mjs:writeFileSync]`

## Verificación

Desde la raíz del repositorio, ejecuta `npm --prefix ui run wasm && npm --prefix ui run build && npm --prefix ui run check:version`.
