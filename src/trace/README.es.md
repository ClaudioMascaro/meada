# Impacto de cambios en el grafo

Calcula los archivos, paquetes y lados de contrato afectados por un cambio para `meada trace --affected`.

## Interfaz

La atribución de lockfiles identifica dependencias modificadas. La selección de raíces relaciona las rutas cambiadas con símbolos, sitios de configuración, nodos IaC y lados de documentos, y la respuesta agrupa los dependientes alcanzados. Un lockfile en un subdirectorio solo puede cubrir manifiestos modificados de miembros dentro de ese directorio. `[affected.rs:attribute]` `[affected.rs:roots]` `[affected.rs:answer]`

## Invariantes

Una ruta modificada sin raíz conocida aparece en `open` con su motivo. Las listas de la respuesta tienen un orden estable. `[affected.rs:roots]` `[affected.rs:answer]`

Cada archivo alcanzado recibe el nivel de la arista más débil de su camino más fuerte (`certain`, `inferred`, `model` o `ambiguous`). Una ejecución derivada de un chart alcanza solo los archivos de su paquete, excluyendo los paquetes anidados. `[affected.rs:Tier]` `[affected.rs:answer]` `[affected.rs:dependents]` `[../trace.rs:runs]`

## Efectos secundarios

La comparación de dependencias puede leer archivos de la copia de trabajo y versiones anteriores a través de Git. La respuesta se construye en memoria. `[affected.rs:attribute]` `[affected.rs:answer]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it -- affected:: kafka_lock::`.
