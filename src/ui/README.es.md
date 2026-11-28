# Operaciones de la API de la consola

Operaciones de la consola para consultar el estado de la plataforma, aplicar cambios y administrar el acceso y los workspaces.

## Interfaz

Los handlers de lectura consultan la flota, el almacenamiento y las versiones. Otras rutas ofrecen catálogo, directorio, inicio de sesión local y operaciones sobre workspaces cuando los módulos están habilitados. `[read.rs:operational]` `[catalog.rs:catalog]` `[directory.rs:directory_refusal]` `[accounts.rs:login]` `[workspaces.rs:workspaces]`

## Invariantes

Las lecturas operativas tienen plazo y límite de concurrencia. Con auditoría en Postgres, una escritura validada registra el estado pendiente antes de modificar Valkey o el almacenamiento. `[read.rs:READERS_MAX]` `[read.rs:operational]` `[write.rs:pending]` `[write.rs:write]`

El inicio de sesión local devuelve HTTP 413 si el cuerpo de la solicitud supera los 4 KiB. `[accounts.rs:LOGIN_MAX]`

## Efectos secundarios

Las rutas usan los servicios habilitados en el manifiesto, incluidos Postgres, Valkey y almacenamiento de objetos. Las operaciones auditadas registran el resultado del intento. `[read.rs:operational]` `[write.rs:operation]` `[write.rs:finish]`

## Componentes

- [Administración de cuentas en la consola](./accounts/README.es.md)
- [Workspaces y propuestas en la consola](./workspaces/README.es.md)

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features cloud --test it ui::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
