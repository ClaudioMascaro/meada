# Objetos de Kubernetes y recursos relacionados

Hechos de Kubernetes para composiciones de Crossplane, recursos personalizados, topología de RabbitMQ, Ingress y entorno de contenedores.

## Interfaz

Los lectores extraen hechos de los objetos, identifican referencias y preparan los nombres físicos y destinos que consultan los tipos de contrato. `[crossplane.rs:facts]` `[custom.rs:physical]` `[topology.rs:Prepared]` `[routing.rs:read]` `[environment.rs:resolve]`

## Invariantes

La resolución de entorno busca objetos por namespace, tipo y nombre en el conjunto recibido. Ingress solo asigna un Service a una ruta `Prefix` literal sin anotaciones de reescritura ni de expresión regular. `[environment.rs:configs]` `[environment.rs:resolve]` `[routing.rs:read]`

Una composición de Crossplane aporta hasta 64 recursos compuestos. Los recursos adicionales quedan fuera del índice. `[crossplane.rs:COMPOSED_MAX]`

## Efectos secundarios

El procesamiento usa objetos ya leídos y transforma las referencias de entorno en memoria al aplicar prefijos, sufijos y namespace del overlay. `[environment.rs:transform]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it kubernetes::`.
