# meada-core

Biblioteca determinística compartilhada pelo binário e pelo console WebAssembly.

## Componentes

- [meada-core · Rust](./src/README.md)

## Verificação

Na raiz do repositório, execute `cargo test -p meada-core`.

## Distribuição

A biblioteca mantém sua própria versão. Nesta release, o binário 1.6.0 depende da biblioteca 2.0.0. O binário exige essa versão exata de `meada-core`. A biblioteca gera `rlib` para o binário e `cdylib` para o WebAssembly, com a feature `wasm-bindgen` no build do navegador. O perfil de desenvolvimento otimiza o pacote `meada` no nível 1 para limitar a memória usada na indexação. `[../Cargo.toml:meada-core]` `[Cargo.toml:crate-type]`
