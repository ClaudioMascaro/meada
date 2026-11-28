# meada-core

Biblioteca determinista compartida por el binario y la consola WebAssembly.

## Componentes

- [meada-core · Rust](./src/README.es.md)

## Verificación

Desde la raíz del repositorio, ejecuta `cargo test -p meada-core`.

## Distribución

La biblioteca mantiene su propia versión. En esta edición, el binario 1.6.0 depende de la biblioteca 2.0.0. El binario exige esa versión exacta de `meada-core`. La biblioteca genera `rlib` para el binario y `cdylib` para WebAssembly, con la feature `wasm-bindgen` en la compilación del navegador. El perfil de desarrollo optimiza el paquete `meada` en el nivel 1 para limitar la memoria utilizada en la indexación. `[../Cargo.toml:meada-core]` `[Cargo.toml:crate-type]`
