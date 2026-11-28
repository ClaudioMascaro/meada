# Cuentas locales en Postgres

Persistencia de las cuentas locales usadas en el inicio de sesión, la administración de la consola y la recuperación de la cuenta predeterminada.

## Interfaz

El intento de inicio de sesión recibe la función que verifica el hash y devuelve un resultado tipado. La creación, los cambios y la consulta de estado usan el nombre de usuario. `[store.rs:attempt]` `[store.rs:Attempt]` `[store.rs:create]` `[store.rs:change]` `[store.rs:standing]`

## Invariantes

La verificación y el recuento de fallos se realizan bajo el mismo bloqueo de fila. Una fila inválida, suspendida o ya bloqueada impide el inicio de sesión. `[store.rs:attempt]`

## Efectos secundarios

Lee y actualiza la tabla `accounts` del esquema configurado en Postgres. La recuperación administrativa restablece la contraseña de la cuenta predeterminada. `[store.rs:attempt]` `[store.rs:reset_admin]`

Tras el inicio de sesión, la actualización del hash solo se escribe si la fila aún contiene el hash verificado. Un cambio de contraseña concurrente prevalece. `[store.rs:rehash]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features accounts --test it ui::accounts::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
