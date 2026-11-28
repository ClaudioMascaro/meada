# Rutas y bibliotecas HTTP de Rust

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Reconoce rutas de axum y describe los constructores y las cadenas de métodos de Rust que acepta la
extracción HTTP.

## Interfaz

`providers` sigue `Router::route` y los métodos que ligan verbos con manejadores. Las tablas
`FRAMEWORKS` y `CLIENT_LIBRARIES` identifican objetos de servidor y clientes reqwest/ureq.
[axum.rs:providers](./axum.rs) [frameworks.rs:FRAMEWORKS](./frameworks.rs) [frameworks.rs:CLIENT_LIBRARIES](./frameworks.rs)

## Invariantes

Una cadena conserva la identidad del receptor solo a través de métodos registrados para el objeto
de origen.
[frameworks.rs:passes_on](./frameworks.rs)

El lector de axum limita a 64 pasos la cadena de métodos que usa para identificar la ruta.
[axum.rs:METHOD_CHAIN](./axum.rs)

## Efectos secundarios

La extracción arma en memoria las rutas y sus manejadores a partir de los hechos del código.
[axum.rs:providers](./axum.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::rust urls::
```
