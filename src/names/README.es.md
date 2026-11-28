# Identificación de nombres externos

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Identifica bibliotecas y constructores por los nombres que permiten resolver las importaciones de
cada lenguaje.

## Interfaz

`imported` identifica el nombre cualificado de una importación de Python. `factory` y
`constructor_argument` recuperan el origen externo del cliente TypeScript y del primer argumento
construido.
[python.rs:imported](./python.rs) [ts.rs:factory](./ts.rs) [ts.rs:constructor_argument](./ts.rs)

`factory_of` lee la construcción de un valor TypeScript en el ámbito donde se escribió.
`field_factory` identifica el cliente de un campo `private` o `#campo` cuando todas las
asignaciones no nulas coinciden en una fábrica. Sigue llamadas a métodos de otro campo privado y
detiene las cadenas cíclicas.
[ts.rs:factory_of](./ts.rs) [ts.rs:field_factory](./ts.rs) [ts.rs:field_factory_in](./ts.rs)

- [Origen de constructores Python](./python/README.es.md)

## Invariantes

Una fábrica TypeScript debe llegar a una importación externa mediante enlaces constantes. El
argumento se examina en su ámbito original antes de la expansión.
[ts.rs:factory](./ts.rs) [ts.rs:constructor_argument](./ts.rs)

## Efectos secundarios

La lectura de opciones invoca al lector recibido con los campos recuperados y devuelve resultados
en memoria conservando los puntos de llamada.
[ts.rs:constructor_options](./ts.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- names:: aws_sdk:: gcp_sdk:: azure_sdk:: kafkajs_classes::
```
