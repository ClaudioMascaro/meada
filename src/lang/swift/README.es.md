# Módulos y valores Swift

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Relaciona archivos Swift con objetivos de SwiftPM y extrae valores escritos en expresiones y
llamadas.

## Interfaz

`targets` lee `Package.swift` con la gramática Swift. `value` interpreta expresiones y `fill`
registra los argumentos y el receptor de una llamada.
[manifest.rs:targets](./manifest.rs) [values.rs:value](./values.rs) [values.rs:fill](./values.rs)

## Invariantes

El nombre de un objetivo debe ser literal. Una ruta explícita calculada, absoluta o con un
segmento `..` excluye el objetivo. Las declaraciones dentro de `dependencies` no crean objetivos.
[manifest.rs:targets](./manifest.rs)

La lectura de valores limita la profundidad de expresiones a 32. En una expresión más profunda, la
parte que supera el límite queda desconocida.
[values.rs:VALUE_DEPTH](./values.rs)

## Efectos secundarios

El analizador devuelve objetivos ordenados por directorio y nombre. La extracción de valores
completa el registro de la llamada en memoria.
[manifest.rs:targets](./manifest.rs) [values.rs:fill](./values.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::swift swiftpkg::
```
