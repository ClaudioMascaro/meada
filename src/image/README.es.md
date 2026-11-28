# Imágenes de contenedor y código ejecutado

Vincula los productores de imágenes con las cargas de trabajo que las ejecutan y con el código local incluido en la construcción.

## Interfaz

El lector resume configuraciones de build, incluidos los destinos de Jib y las entradas del Dockerfile. La preparación resuelve patrones de nombres de imagen y la proyección de entorno conserva el contenedor que ejecuta el código. `[config.rs:read]` `[jib.rs:gradle]` `[dockerfile.rs:executions]` `[patterns.rs:prepare]` `[environment.rs:scoped_values]`

Con `[[helm_env]]`, el lector extrae las entradas escalares de la clave declarada en charts con `Chart.yaml`. Cada `values-<env>.yaml` o `.yml` sobrescribe los valores de la base. Sin variantes, la base forma un único entorno. `[config.rs:chart_envs]` `[config.rs:laid]`

## Invariantes

Cuando no puede determinar las entradas del Dockerfile, el lector puede atribuir el contexto entero como evidencia inferida. Las rutas de código verificadas permanecen dentro del repositorio. `[dockerfile.rs:executions]` `[code.rs:checked]`

Solo el paquete propietario del chart recibe ese entorno para resolver canales. Si todos los entornos de los charts que definen la variable coinciden en el literal, la evidencia puede alcanzar `declared`. Un valor ausente en uno de esos entornos o valores distintos limitan los resultados a `inferred`. `[environment.rs:entries]` `[environment.rs:values]` `[environment.rs:resolve]`

## Efectos secundarios

La indexación comprueba rutas en la copia de trabajo y lee el Dockerfile. La preparación de imágenes candidatas actualiza las exportaciones en memoria. `[code.rs:checked]` `[dockerfile.rs:executions]` `[patterns.rs:prepare]`

El lector enumera los archivos del chart y lee la base del repositorio local. Si se supera el presupuesto por archivo, omite el builder en curso o descarta todo el entorno de Helm. `[config.rs:chart_envs]` `[config.rs:builder]` `[config.rs:IMAGE_STORED_MAX]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it -- images:: helm_env::`.
