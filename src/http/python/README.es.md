# Clientes HTTP de Python y Rust

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Comparte la lectura de llamadas HTTP entre requests, httpx, aiohttp y las tablas de Rust para
reqwest y ureq.

## Interfaz

`consumers` recibe hechos del código, identifica funciones del módulo o métodos de un cliente y
expande envoltorios en los puntos de llamada.
[clients.rs:consumers](./clients.rs)

## Invariantes

El origen del receptor identifica la biblioteca. En aiohttp una ruta que empieza por `/` sustituye
la ruta base aunque esta sea desconocida. Una referencia relativa que depende de esa base queda
abierta.
[clients.rs:consumers](./clients.rs)

## Efectos secundarios

El resultado es un vector de consumidores construido en memoria a partir de los hechos del código.
[clients.rs:consumers](./clients.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::python urls::
```
