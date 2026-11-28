# Persistencia y ciclo de vida de workspaces

Persistencia de workspaces con nombre y comandos administrativos para creación, edición, aplicación de versiones e historial.

## Interfaz

Las operaciones de la consola y la CLI comparten la creación y las transacciones de ciclo de vida. La creación registra el workspace, su primer operador y la versión inicial. La aplicación y la reversión cambian la versión vigente. `[store.rs:create]` `[lifecycle.rs:transact]` `[cli.rs:create]` `[cli.rs:apply]`

## Invariantes

La creación comprueba la elegibilidad en el conjunto vigente de permisos antes de registrar las filas SQL. Los cambios de ciclo de vida revalidan la versión base y la autorización bajo el bloqueo de fila. `[store.rs:eligible]` `[store.rs:create]` `[lifecycle.rs:transact]`

Al cambiar la lista, el workspace debe conservar como mínimo un operador y como máximo 20. `[lifecycle.rs:OPERATORS_MAX]` `[lifecycle.rs:transact]`

## Efectos secundarios

Las tablas de estado e historial residen en Postgres. Los documentos de versión inmutables usan el almacenamiento de objetos. La CLI audita las escrituras e informa del resultado en la terminal. `[store.rs:create]` `[store.rs:put_document]` `[lifecycle.rs:transact]` `[cli.rs:apply]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features workspaces --test it ui::oidc::workspaces::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
