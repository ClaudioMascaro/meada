# Rutas y clientes HTTP de Swift

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Lee rutas de Vapor y Hummingbird y llamadas de clientes Swift.

## Interfaz

`providers` usa un dialecto para leer grupos y manejadores de ambos frameworks. `consumers`
reconoce URLSession, Alamofire y AsyncHTTPClient.
[vapor.rs:providers](./vapor.rs) [hummingbird.rs:HUMMINGBIRD](./hummingbird.rs) [client.rs:consumers](./client.rs)

## Invariantes

Los prefijos pertenecen al grupo que registra la ruta. Del lado cliente, el origen del receptor
debe identificar la biblioteca.
[vapor.rs:providers](./vapor.rs) [client.rs:consumers](./client.rs)

La búsqueda del origen del cliente sigue hasta ocho niveles de nombres locales y constantes.
[client.rs:CLIENT_DEPTH](./client.rs)

## Efectos secundarios

Los lectores recorren los hechos Swift disponibles y devuelven rutas y consumidores en memoria.
[vapor.rs:providers](./vapor.rs) [client.rs:consumers](./client.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::swift swiftpkg::
```
