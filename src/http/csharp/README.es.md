# Rutas y clientes HTTP de C#

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Identifica emisores de solicitudes HttpClient y Refit y rutas de Minimal APIs en ASP.NET Core.

## Interfaz

`providers` relaciona `MapGroup` y `MapMethods` con rutas. Los dos lectores `consumers` distinguen
llamadas de HttpClient y métodos declarados en interfaces Refit.
[minimal.rs:providers](./minimal.rs) [clients.rs:consumers](./clients.rs) [refit.rs:consumers](./refit.rs)

## Invariantes

En HttpClient una ruta que empieza por `/` sustituye la ruta base. En Refit la ruta anotada
conserva la ruta base. Una lista parcialmente desconocida de verbos en `MapMethods` impide el
registro.
[clients.rs:rebase](./clients.rs) [refit.rs:url](./refit.rs) [minimal.rs:methods](./minimal.rs)

Cuando el grafo de grupos de Minimal APIs supera los 4096 nodos, el lector omite sus rutas para
evitar publicar una lista incompleta de prefijos.
[minimal.rs:MINIMAL_NODES](./minimal.rs)

## Efectos secundarios

`Reads` guarda en memoria la configuración indexada y carga los valores bajo demanda. La lectura
de una variable de entorno conserva su nombre.
[reads.rs:Reads](./reads.rs) [reads.rs:read](./reads.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::csharp nugetpkg::
```
