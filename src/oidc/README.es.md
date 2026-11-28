# Grupos de Entra ID desde el directorio

Resuelve los grupos de Entra ID cuando el token indica exceso de grupos y el manifiesto selecciona el directorio.

## Interfaz

El detector comprueba que falta el claim de grupos y que aparece en `_claim_names`. La consulta usa el claim de usuario para recuperar los grupos del usuario activo en el directorio. `[overage.rs:in_overage]` `[overage.rs:groups]`

## Invariantes

La consulta usa únicamente el directorio configurado. Un usuario ausente o inactivo recibe cero grupos y una consulta fallida no entra en la caché. `[overage.rs:ask]` `[overage.rs:groups]`

## Efectos secundarios

Consulta Postgres y guarda en memoria durante cinco minutos por defecto las respuestas correctas, incluidas las vacías. Un fallo de consulta emite un diagnóstico. `[overage.rs:groups]` `[overage.rs:OVERAGE_TTL]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features oidc,ui,directory --test it oidc::overage::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
