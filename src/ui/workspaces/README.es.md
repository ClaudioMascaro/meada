# Workspaces y propuestas en la consola

Operaciones de la consola sobre workspaces con nombre y servidor de propuestas para agentes.

## Interfaz

La consola ofrece lectura de versiones e historial, configuración efectiva, vista previa y cambios de ciclo de vida. Con la feature `proposals`, el servidor MCP expone catálogo, descubrimiento, consulta y propuesta. `[effective.rs:configuration]` `[effective.rs:preview]` `[lifecycle.rs:operate]` `[lifecycle.rs:inspect]` `[proposals.rs:listen]`

## Invariantes

La sesión determina la identidad que se usa para autorizar. Las lecturas filtran las entradas según los permisos del usuario y la vista previa de configuración no guarda una versión. `[lifecycle.rs:readable_entries]` `[effective.rs:preview]`

El servidor MCP de propuestas devuelve HTTP 413 cuando el cuerpo de la solicitud supera los 80 KiB. `[proposals.rs:BODY_MAX]`

## Efectos secundarios

Las operaciones de ciclo de vida delegan las escrituras al almacenamiento de workspaces. El servidor de propuestas abre un listener HTTP y evalúa propuestas sin persistirlas. `[lifecycle.rs:operate]` `[proposals.rs:listen]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --features cloud --test it ui::oidc::workspaces::`.

Para ejercitar los casos que usan servicios, cargue las variables que imprime `scripts/test-services up` antes de la prueba.
