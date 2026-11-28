# Extracción de los SDK de Google Cloud

Convierte evidencia de clientes de Google Cloud en contratos de Pub/Sub, Cloud Storage y Secret Manager.

## Interfaz

La extracción selecciona el lector del lenguaje, sigue el receptor y emite el lado correspondiente a la operación. La proyección de entorno distingue nombres de temas y suscripciones. `[sdk.rs:extract]` `[sdk.rs:environment_name]` `[sdk.rs:environment_target]`

## Invariantes

Obtener un cliente o identificador de tema aún no define un productor. El lector conserva la etapa hasta la llamada que usa el destino. `[sdk.rs:extract]`

## Efectos secundarios

Produce una colección en memoria a partir de los archivos ya analizados. `[sdk.rs:extract]`

## Componentes

- [Llamadas a los SDK de Google Cloud por lenguaje](./sdk/README.es.md)

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it gcp_sdk::`.
