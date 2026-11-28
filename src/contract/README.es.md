# Identidades y extracción de contratos

Reglas de identidad física y mecanismos de extracción compartidos por los tipos de contrato.

## Interfaz

Los normalizadores de GCP y Azure convierten los nombres admitidos en identidades que utilizan la configuración, IaC y los SDK. La extracción compartida combina clientes, operaciones y argumentos. `[gcp.rs:pubsub]` `[azure.rs:servicebus]` `[sdk.rs:extract]`

La identidad persistida de un receptor registra el tipo y los argumentos de un SDK o las eliminaciones de barras y el sufijo de una URL base HTTP. `[receiver.rs:ReceiverIdentity]`

## Invariantes

Una suscripción de Pub/Sub identifica la propia suscripción. Al combinar valores de entorno, el ámbito identifica la carga de trabajo, la raíz y el contenedor. `[gcp.rs:pubsub]` `[receiver.rs:EnvironmentScope]`

La normalización de identificadores ARM acepta de 1 a 1024 bytes. `[azure.rs:arm]` `[azure.rs:AZURE_IDENTIFIER_MAX]`

## Efectos secundarios

Los extractores reciben hechos del código y devuelven los lados encontrados en memoria. `[sdk.rs:extract]`

## Componentes

- [Extracción de los SDK de Azure](./azure/README.es.md)
- [Extracción de los SDK de Google Cloud](./gcp/README.es.md)
- [Origen de clientes y argumentos de SDK](./sdk/README.es.md)

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it -- aws_sdk:: azure_sdk:: gcp_sdk::`.
