# Extracción de contratos HTTP

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Reúne rutas servidas, llamadas consumidoras y rutas publicadas por la infraestructura bajo la
identidad `http::<METHOD>::<path>`.

## Interfaz

Los adaptadores de lenguaje devuelven proveedores y consumidores a partir de los hechos del
código. El lector de IaC declara rutas desde la infraestructura evaluada. `prepare` combina
Services, Ingresses y cargas de trabajo para explicar enlaces.
[../http.rs:Http](../http.rs) [iac.rs:declare](./iac.rs) [infrastructure.rs:prepare](./infrastructure.rs)

Los proveedores incluyen Express y archivos de ruta de Next.js, FastAPI y Flask, enrutadores de
Go, Spring MVC/JAX-RS, Ktor, controladores ASP.NET, actix-web/Rocket y Vapor/Hummingbird.
[ts.rs:providers](./ts.rs) [python.rs:providers](./python.rs) [go.rs:providers](./go.rs) [java.rs:providers](./java.rs) [kotlin.rs:providers](./kotlin.rs) [csharp.rs:providers](./csharp.rs) [rust.rs:providers](./rust.rs) [swift.rs:providers](./swift.rs)

- [Rutas y clientes HTTP de C#](./csharp/README.es.md)
- [Clientes HTTP de Go](./go/README.es.md)
- [Clientes HTTP de la JVM](./java/README.es.md)
- [Clientes HTTP Ktor](./kotlin/README.es.md)
- [Clientes HTTP de Python y Rust](./python/README.es.md)
- [Rutas y bibliotecas HTTP de Rust](./rust/README.es.md)
- [Rutas y clientes HTTP de Swift](./swift/README.es.md)
- [Rutas y consumidores JavaScript y TypeScript](./ts/README.es.md)

## Invariantes

La identidad usa el método y la ruta normalizada. Las URL relativas siguen la resolución de
referencias RFC 3986 en los clientes que la adoptan. Los enlaces por infraestructura dependen de
las rutas evaluadas.
[../http.rs:contract](../http.rs) [join.rs:resolve](./join.rs) [infrastructure.rs:prepare](./infrastructure.rs)

Una variable de `[[base_url]]` restringe los proveedores al directorio declarado en el mismo
repositorio. Para una URL relativa, si el paquete del consumidor sirve la ruta, solo se conservan
los proveedores de ese paquete. Un enlace que depende de la reescritura de la pasarela queda como
máximo en `inferred`.
[../http.rs:link](../http.rs) [../http.rs:under](../http.rs)

Si la lista serializada de rutas IaC supera el presupuesto del archivo, el lector descarta todo el
resumen. En las URL aportadas por el entorno de una carga, la preparación vuelve a aplicar la
eliminación explícita de barras finales antes de concatenar el sufijo escrito por el cliente.
[iac.rs:IAC_HTTP_STORED_MAX](./iac.rs) [infrastructure.rs:workload_url](./infrastructure.rs)

## Efectos secundarios

La preparación construye en memoria los enlaces y sus explicaciones para la consulta.
[infrastructure.rs:Prepared](./infrastructure.rs) [infrastructure.rs:prepare](./infrastructure.rs)

`prepare` también sustituye consumidores por sus variantes resueltas a partir de las cargas,
ordena los resultados, elimina duplicados y añade rutas de pasarela a los proveedores.
[infrastructure.rs:prepare](./infrastructure.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http:: http_iac:: urls:: test_routes:: base_url:: http_bases:: helm_env::
```
