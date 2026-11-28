# Directorio SCIM

Directorio de usuarios y grupos aprovisionados por SCIM, consultado por la consola para la admisión y selección de responsables.

## Interfaz

La interfaz SCIM gestiona usuarios, grupos y metadatos en `/scim/v2/`. El lector de la consola consulta bajas, pertenencia a grupos y recuentos. `[scim.rs:Scim]` `[scim.rs:answer]` `[read.rs:Directory]` `[read.rs:deprovisioned]` `[read.rs:counts]`

## Invariantes

La admisión SCIM valida la credencial de aprovisionamiento. El tamaño del cuerpo y de las páginas está limitado. `[scim.rs:admits]` `[scim.rs:SCIM_BODY_MAX]` `[scim.rs:SCIM_PAGE_MAX]`

Cada cambio debe mantener el JSON del recurso almacenado dentro de 64 KiB. Superar ese límite devuelve HTTP 413 incluso para un PATCH pequeño. `[scim.rs:candidate]` `[scim.rs:SCIM_RESOURCE_MAX]`

## Efectos secundarios

El aprovisionamiento modifica los registros de usuarios, grupos y miembros en Postgres. Las consultas de la consola abren una conexión para leer el estado vigente. `[scim.rs:answer]` `[read.rs:groups]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features ui,oidc,directory --test it ui::oidc::directory::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
