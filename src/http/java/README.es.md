# Clientes HTTP de la JVM

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Reconoce clientes Java imperativos e interfaces declarativas que envían solicitudes.

## Interfaz

Un lector sigue RestTemplate, WebClient, `java.net.http` y OkHttp. Otro registra los métodos
OpenFeign y Retrofit en la definición de la interfaz, donde el grafo puede alcanzar al consumidor.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

## Invariantes

Las importaciones y los tipos identifican el framework. En Retrofit cada `create` puede aportar
una base. Una ruta relativa con base desconocida queda abierta.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

En `@FeignClient`, una URL que empieza por texto literal sin `://`, como `localhost:8080`, recibe
el prefijo `http://`.
[declarative.rs:schemed](./declarative.rs)

## Efectos secundarios

Los lectores consultan los hechos y la configuración indexada de Spring para armar los resultados
en memoria.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::java jvmpkg::
```
