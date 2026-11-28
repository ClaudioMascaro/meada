# Documentos OpenAPI y AsyncAPI

Documentos OpenAPI y AsyncAPI como fuentes de contratos y formas de payload, con un lector compartido de eventos YAML y JSON.

## Interfaz

La entrada recibe los bytes de un documento y produce un resumen de rutas HTTP o lados de canales. Los lectores de eventos también exponen las posiciones de escalares para otros lectores de configuración. `[mod.rs:read]` `[openapi.rs:read]` `[asyncapi.rs:read]` `[events.rs:read_located]`

## Invariantes

Las referencias de esquema se resuelven dentro del documento. El lector limita el trabajo y deja abiertas las partes de la forma que no puede enumerar. `[schema.rs:read]` `[schema.rs:Work]`

El lector de eventos rechaza documentos con más de 128 niveles de anidamiento y cuenta los nodos de un alias a la profundidad en que se insertan. `[events.rs:MAX_NESTING]`

## Efectos secundarios

Los lectores construyen resultados en memoria a partir de los bytes y entregan eventos a las funciones suministradas. `[mod.rs:read]` `[events.rs:read]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it documents::`.
