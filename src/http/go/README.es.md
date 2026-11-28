# Clientes HTTP de Go

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Convierte llamadas de `net/http` y Resty en consumidores HTTP.

## Interfaz

`consumers` recibe los hechos del código y devuelve el método y el destino de cada llamada
reconocida. `is_package_send` identifica envíos del paquete.
[clients.rs:consumers](./clients.rs) [clients.rs:is_package_send](./clients.rs)

## Invariantes

El receptor necesita una importación, un tipo declarado o una construcción que lo vincule con la
biblioteca. La URL base de Resty sigue a su instancia aunque una fábrica cree dos clientes.
[clients.rs:consumers](./clients.rs)

Al resolver el cliente almacenado en un campo, una lectura en producción descarta los valores
aportados solo por pruebas. Una lectura en un archivo de prueba considera ambos. La búsqueda de
orígenes se detiene en 64 pasos y puede dejar el cliente desconocido.
[clients.rs:held](./clients.rs) [clients.rs:CLIENT_DEPTH](./clients.rs) [../../resolve.rs:field_values](../../resolve.rs)

## Efectos secundarios

La extracción reúne en memoria los consumidores a partir de las llamadas y sus bases.
[clients.rs:consumers](./clients.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::go gopkg:: base_url::
```
