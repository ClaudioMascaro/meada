# Rutas y consumidores JavaScript y TypeScript

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Relaciona llamadas a `fetch` con rutas y con la configuración de plugins y proxies de Backstage.

## Interfaz

`consumers` lee URL, métodos y campos de respuesta. `plugins` y `discovered` identifican el
montaje y el descubrimiento de plugins. `Config` resuelve lecturas de configuración y destinos de
proxy.
[clients.rs:consumers](./clients.rs) [backstage.rs:plugins](./backstage.rs) [backstage.rs:discovered](./backstage.rs) [config.rs:Config](./config.rs)

`Config` lee cada base distinta de `app-config*.yaml` y de sus inclusiones. Una clave dinámica de
un objeto puede dar varios destinos. La expansión de `fetch` conserva el punto de la petición
cuando un método auxiliar devuelve la base de descubrimiento. Un `pluginId` de
`createBackendPlugin` o `createBackendModule` también puede proceder de una constante del mismo
archivo.
[config.rs:decided](./config.rs) [backstage.rs:discovered](./backstage.rs) [backstage.rs:plugins](./backstage.rs) [clients.rs:consumers](./clients.rs)

`Config::literals` devuelve los valores distintos de una clave en orden determinista. `whole`
indica si se leyeron todas las raíces e inclusiones. Un valor compuesto únicamente por `${VAR}`
conserva el nombre de la variable para resolver la base mediante la configuración de la carga.
[config.rs:literals](./config.rs) [config.rs:whole](./config.rs) [config.rs:configured](./config.rs)

- [Inclusiones de configuración de Backstage](./config/README.es.md)

## Invariantes

`fetch` cuenta solo donde el paquete de lenguaje lo declara global, incluidos los parámetros con
`fetch` como valor predeterminado. Un plugin con ID literal tiene el montaje `/api/<id>`.
[clients.rs:consumers](./clients.rs) [backstage.rs:mount](./backstage.rs)

Varias bases, una configuración incompleta o la selección dinámica de un registro limitan los
enlaces a `inferred`. Si no se puede leer el destino del proxy, el consumidor conserva el nombre
del endpoint e indica que su destino no pudo leerse.
[config.rs:decided](./config.rs) [config.rs:configured](./config.rs) [clients.rs:consumers](./clients.rs) [backstage.rs:proxied](./backstage.rs)

## Efectos secundarios

La configuración se analiza bajo demanda. Las inclusiones que ninguna raíz siguió generan
registros con el motivo del rechazo.
[config.rs:Config](./config.rs) [config.rs:unfollowed](./config.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- http::ts urls:: http_bases::
```
