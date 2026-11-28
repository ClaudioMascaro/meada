# Paquetes en el manifiesto SwiftPM

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Lee la parte de `Package.swift` que identifica el paquete, sus productos de biblioteca y sus
dependencias.

## Interfaz

`package` recibe los bytes del manifiesto y devuelve `Package` con productos, objetivos de
biblioteca y entradas `Dependency`, incluidas la ruta o URL y la condición de versión.
[manifest.rs:package](./manifest.rs) [manifest.rs:Package](./manifest.rs) [manifest.rs:Dependency](./manifest.rs)

## Invariantes

Una dependencia debe declarar exactamente uno entre `url` y `path`. Las partes calculadas quedan
marcadas con `*` y el indicador `computed`.
[manifest.rs:package](./manifest.rs) [manifest.rs:Dependency](./manifest.rs)

## Efectos secundarios

El analizador recorre el árbol local en orden de código y conserva el primer nombre encontrado en
`Package(name:)`. El resultado queda en memoria.
[manifest.rs:package](./manifest.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- package::swiftpm swiftpkg::
```
