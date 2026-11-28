# Permisos de acceso de GitHub

Permisos de lectura derivados del conjunto de permisos GitHub más reciente en Postgres.

## Interfaz

El lector mantiene los permisos por usuario y devuelve los permisos y el estado del conjunto para la admisión. Con workspaces también expone el catálogo disponible para selección. `[permissions.rs:Permissions]` `[permissions.rs:grant_and_state]` `[permissions.rs:catalog]`

## Invariantes

Antes de la primera lectura válida y al vencer el conjunto, los permisos GitHub quedan vacíos. La antigüedad combina el reloj de Postgres en la lectura con el tiempo monotónico local transcurrido. `[permissions.rs:grant_and_state]` `[permissions.rs:admission]`

Por defecto, el lector consulta nuevos conjuntos cada 60 segundos. `max_age_seconds` determina cuánto tiempo puede conceder acceso el último conjunto. `[permissions.rs:PERMISSIONS_POLL]` `[permissions.rs:grant_and_state]`

## Efectos secundarios

La actualización periódica consulta Postgres en una transacción de lectura y sustituye el estado en memoria. La admisión consulta ese estado local. `[permissions.rs:poll]` `[permissions.rs:grant_and_state]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features workspaces --test it github_app::roles::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
