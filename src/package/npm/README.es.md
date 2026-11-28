# Registro de plugins npm en Backstage

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Identifica paquetes cargados como plugins en un backend de Backstage o como features en un backend
de prueba.

## Interfaz

`added_plugin` devuelve el paquete pasado a `backend.add(import(...))`. `test_features` extrae las
importaciones de la lista `features` entregada a `startTestBackend`.
[backstage.rs:added_plugin](./backstage.rs) [backstage.rs:test_features](./backstage.rs)

## Invariantes

El backend debe tener un único origen constante vinculado a `createBackend` del paquete esperado.
Solo un `import()` literal identifica el plugin.
[backstage.rs:made_by](./backstage.rs) [backstage.rs:added_plugin](./backstage.rs)

`added_plugin` exige un nombre simple como receptor de `.add` y exactamente un argumento. Omite
`app.backend.add(...)` y llamadas con argumentos adicionales.
[backstage.rs:added_plugin](./backstage.rs)

## Efectos secundarios

Los lectores consultan las importaciones y los argumentos indexados y devuelven los nombres
encontrados en memoria.
[backstage.rs:added_plugin](./backstage.rs) [backstage.rs:test_features](./backstage.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- package::npm packages::
```
