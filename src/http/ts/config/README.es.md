# Inclusiones de configuración de Backstage

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Expande `$include` y `$env` para poder leer las URL desde los archivos de configuración.

## Interfaz

`tree` recibe los bytes de la raíz, un lector de archivos del repositorio, las rutas observadas y
un presupuesto compartido. Devuelve el árbol y los motivos de las inclusiones rechazadas.
[includes.rs:tree](./includes.rs) [includes.rs:Reason](./includes.rs)

## Invariantes

Solo se pueden incluir archivos JSON o YAML dentro del repositorio. La expansión rechaza ciclos y
limita la profundidad a ocho y las inclusiones a 64 por raíz.
[includes.rs:tree](./includes.rs) [includes.rs:DEPTH](./includes.rs) [includes.rs:INCLUDES](./includes.rs)

La expansión también contabiliza los bytes incorporados por raíz y en la lectura compartida entre
raíces. Incluir dos veces el mismo archivo consume presupuesto en ambas ocasiones.
[includes.rs:GRAFTED_MAX](./includes.rs) [includes.rs:READING_MAX](./includes.rs)

## Efectos secundarios

El lector solicita los archivos a través de la función recibida y descuenta las lecturas del
presupuesto compartido. `$env` conserva el nombre de la variable.
[includes.rs:tree](./includes.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::ts::config urls:: http_bases::
```
