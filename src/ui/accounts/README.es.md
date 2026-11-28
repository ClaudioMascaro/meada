# Administración de cuentas en la consola

Administración de cuentas locales y vinculación de un usuario verificado de GitHub desde la consola.

## Interfaz

El operador global lista, crea, modifica o suspende cuentas. Cada persona puede vincular o quitar su propio usuario de GitHub cuando la integración está configurada. `[administration.rs:administer]` `[github.rs:verify_github]` `[github.rs:remove_own_github]`

## Invariantes

La identidad de GitHub procede de la respuesta a `GET /user`. El listado administrativo omite los hashes de contraseña. `[github.rs:Verifier]` `[administration.rs:administer]`

Las solicitudes para crear o modificar cuentas tienen un límite de 4 KiB y reciben HTTP 413 si lo superan. `[administration.rs:ADMIN_BODY_MAX]`

## Efectos secundarios

La verificación consulta la API configurada con el token suministrado. Los cambios actualizan las cuentas en Postgres y pasan por la auditoría de la consola. `[github.rs:verify_github]` `[administration.rs:administer]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features accounts,github-app --test it ui::accounts::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
