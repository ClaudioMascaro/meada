# Extracción de los SDK de Azure

Convierte llamadas a los SDK de Azure en lados de contratos de canal o recurso.

## Interfaz

La política conserva la identidad del cliente hasta la operación de Service Bus, Event Hubs, Blob Storage o Key Vault y devuelve los resultados de la familia solicitada. `[sdk.rs:extract]`

## Invariantes

La extracción exige evidencia del receptor. Un constructor o nombre de método aislado no basta para atribuir dirección al contrato. `[sdk.rs:extract]`

## Efectos secundarios

Acumula en memoria los lados encontrados a partir de los hechos de cada archivo. `[sdk.rs:extract]`

## Componentes

- [Llamadas a los SDK de Azure por lenguaje](./sdk/README.es.md)

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it azure_sdk::`.
