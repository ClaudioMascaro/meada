# Distribución de snapshots y generaciones

Distribución de generaciones de índices, retención de versiones y coordinación entre publicadores y nodos de lectura.

## Interfaz

El cliente de almacenamiento transfiere objetos. El pull instala generaciones en el nodo y el servicio lee la generación local. Las features correspondientes añaden puntero y retención en Valkey, heartbeat, bloqueo de actualización y operaciones de versión. `[store.rs:Store]` `[pull.rs:pull]` `[serve.rs:Current]` `[generations.rs:point]` `[heartbeat.rs:beat]` `[lock.rs:acquire]` `[versions.rs:apply]`

## Invariantes

Los objetos descargados se validan antes de cambiar el puntero local. Con el bloqueo de actualización, la publicación necesita el token vigente y un contador mayor que el registrado en el puntero para mover el puntero compartido. `[pull.rs:pull]` `[generations.rs:point]`

La lectura del puntero en Valkey valida los identificadores y contadores y rechaza una lista de retención vacía o con más de `keep + 8` entradas. Si el `meada.toml` del espejo es inválido, la consulta de configuración efectiva sigue devolviendo `held = true` y `own = true`, con `documents = false`. `[generations.rs:read]` `[generations.rs:KEPT_MAX]` `[serve.rs:facts]` `[../config.rs:read_config_up_to]`

## Efectos secundarios

Transfiere objetos por S3, escribe archivos locales y sustituye el archivo del puntero mediante un cambio de nombre. Las funciones de coordinación modifican claves en Valkey y el comando de bloqueo también ejecuta el proceso hijo solicitado. `[store.rs:Store]` `[pull.rs:replace]` `[operation.rs:pointer_set]` `[heartbeat.rs:beat]` `[lock.rs:run]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features refresh-lock --test it -- snapshot:: generations::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
