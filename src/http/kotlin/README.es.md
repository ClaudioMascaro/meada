# Clientes HTTP Ktor

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Extrae consumidores de llamadas Kotlin al `HttpClient` de Ktor.

## Interfaz

`consumers` identifica el verbo importado, el origen del receptor y la URL que pasa quien llama.
También considera la base de `defaultRequest`.
[client.rs:consumers](./client.rs)

## Invariantes

Un `request` sin asignación de método usa GET. Una asignación condicional deja el método
desconocido. Una URL definida solo en el bloque constructor queda sin resolver.
[client.rs:consumers](./client.rs)

## Efectos secundarios

Los valores inyectados proceden de la configuración indexada del módulo. El lector devuelve los
consumidores en memoria.
[client.rs:consumers](./client.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::kotlin kotlinpkg::
```
