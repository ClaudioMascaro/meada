# Catálogo de instalaciones de GitHub

Descubrimiento de identidades de repositorio candidatas a la comprobación de roles de GitHub App.

## Interfaz

A partir de las instalaciones de los propietarios configurados y de la selección existente, produce un conjunto ordenado de identidades para la etapa de autorización. `[catalog.rs:candidates]`

## Invariantes

La inclusión en el catálogo solo convierte el repositorio en candidato. Los fallos de paginación o consulta interrumpen el listado para impedir que un resultado parcial sustituya el conjunto anterior. `[catalog.rs:candidates]` `[catalog.rs:repositories]`

Si la última página permitida llega llena, la consulta falla porque aún puede haber repositorios por leer. `[catalog.rs:repositories]`

## Efectos secundarios

Consulta la API de repositorios de la instalación con su token y usa las marcas de limitación de llamadas compartidas por el cliente. `[catalog.rs:repositories]` `[catalog.rs:candidates]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features workspaces --test it github_app::roles::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
