# Recursos usados por el código o declarados en la configuración

Contratos de recursos a partir de llamadas a los SDK de AWS y de ubicaciones declaradas en la configuración indexada.

## Interfaz

La tabla de operaciones indica el servicio, la clave del nombre y la dirección. Los lectores de boto3 y JS/TS identifican el cliente y sus argumentos. El lector de declaraciones resuelve los selectores de configuración en sitios del índice. `[aws.rs:OPERATIONS]` `[aws.rs:extract]` `[python.rs:sides]` `[ts.rs:sides]` `[via.rs:read]`

La consulta inversa por ubicación muestra los recursos que proporciona o consume, los lectores o escritores correspondientes y la evidencia de cada vínculo. `[via.rs:context]`

## Invariantes

En JS/TS el cliente y el comando deben proceder del mismo paquete del SDK previsto para la operación. En Python la fábrica del cliente debe corresponder al servicio. `[ts.rs:sides]` `[python.rs:sides]`

Al vincular un lector y un escritor, el vínculo conserva el nivel de evidencia más débil de los dos lados. Un nombre obtenido del entorno de ejecución limita el vínculo a `inferred` si el lado no registra otro límite. `[../resource.rs:link]` `[../contract.rs:environment_cap]`

## Efectos secundarios

La resolución de declaraciones consulta SQLite y añade los sitios encontrados a la exportación en memoria. El análisis del SDK usa hechos extraídos previamente. `[via.rs:read]` `[aws.rs:extract]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it resources::`.
