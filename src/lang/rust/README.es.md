# Módulos, macros y valores de Rust

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Reconstruye el árbol de módulos de Cargo y los hechos sintácticos usados para resolver valores y
referencias en código Rust.

## Interfaz

Los lectores reconstruyen módulos y `cfg`, reconocen macros de elementos que conservan los
elementos, identifican menciones y extraen valores, incluidas cadenas formadas por adiciones
sucesivas.
[crates.rs:Tree](./crates.rs) [cfg.rs:holds](./cfg.rs) [macros.rs:bodies](./macros.rs) [mentions.rs:mentions](./mentions.rs) [values.rs:Reader](./values.rs) [builders.rs:builder](./builders.rs)

## Invariantes

La configuración usa Linux x86_64 y las features predeterminadas de la crate. Los predicados mal
formados o demasiado profundos son falsos. La búsqueda de módulos usa solo los archivos presentes
en el inventario.
[cfg.rs:holds](./cfg.rs) [cfg.rs:defaults](./cfg.rs) [crates.rs:Tree](./crates.rs)

Los constructores de cadenas comparten un presupuesto de trabajo por archivo. Cuando se agota, las
construcciones restantes dan valores desconocidos.
[builders.rs:BUILDER_BUDGET](./builders.rs) [builders.rs:builder](./builders.rs)

## Efectos secundarios

`record` añade a los hechos del archivo las definiciones e invocaciones de macros pertinentes para
un nuevo análisis de los elementos.
[macros.rs:record](./macros.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::rust cargo::
```
