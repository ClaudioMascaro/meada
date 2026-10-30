# Roadmap

S1, S2, S4 y S5 están entregados, con la medición de S5 contra una organización real ([#338](https://github.com/ClaudioMascaro/meada/issues/338)) todavía abierta. S3 sigue planeado. Este mapa muestra dependencias y límites de alcance, sin prometer fechas.

<!-- meada-graph:roadmap -->

## Qué está disponible

[S1, #259](https://github.com/ClaudioMascaro/meada/issues/259) entrega las bases del [modo servidor](server.md). [S2, #293](https://github.com/ClaudioMascaro/meada/issues/293) entrega la [consola](console.md), versiones inmutables y operaciones de la flota.

## Bases en construcción

[S3, #294](https://github.com/ClaudioMascaro/meada/issues/294) trata owners y autorización de entradas. [S4, #319](https://github.com/ClaudioMascaro/meada/issues/319), ya entregado como [identidad empresarial](enterprise.md), trata identidad, grants, directorio y auditoría. Partes de estas bases pueden avanzar en paralelo; el grafo de tickets determina los bloqueos reales. El mapa no presenta todo S3 como requisito previo de todo S4.

S5 depende de ambas bases, de la [publicación atómica, #316](https://github.com/ClaudioMascaro/meada/issues/316), y de la [medición y validación, #338](https://github.com/ClaudioMascaro/meada/issues/338).

## Después de las bases

[S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) introduce [workspaces con nombre](named-workspaces.md) sobre indexación compartida. La [consolidación de contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), está terminada (ADR 0020 a 0022) y precede al runtime. La secuencia aceptada es contratos → catálogo elegible → primer workspace → versiones y superposición → configuración entre repositorios. El catálogo elegible ([#341](https://github.com/ClaudioMascaro/meada/issues/341)) está entregado: descubrir los repositorios seleccionables antes de que exista un manifiesto, sin crear workspace ni índice. El primer workspace ([#342](https://github.com/ClaudioMascaro/meada/issues/342)) también está entregado: creación desde el catálogo, ingestión compartida con la regla del patrocinador y la ruta de nodo `/-/workspaces/<id>/<identity>`; el ciclo de vida de las versiones ([#343](https://github.com/ClaudioMascaro/meada/issues/343)) también está entregado: guardar, aplicar, revertir, renombrar, borrar y delegar, con una base que mantiene separados los workspaces superpuestos y a los editores simultáneos; la configuración por workspace ([#344](https://github.com/ClaudioMascaro/meada/issues/344)) también está entregada, y la autoría guiada ([#345](https://github.com/ClaudioMascaro/meada/issues/345)) también: controles de formulario para repositorios, peers, ajustes de serve, recursos y nombres de variables de entorno que siguen siendo un solo documento con el texto del manifiesto. La medición contra una organización real de GitHub ([#338](https://github.com/ClaudioMascaro/meada/issues/338)) sigue pendiente; el mantenedor decidió que no condiciona S5. Las propuestas MCP autenticadas ([#346](https://github.com/ClaudioMascaro/meada/issues/346)) también están entregadas, y también la validación de extremo a extremo con el procedimiento de actualización ([#347](https://github.com/ClaudioMascaro/meada/issues/347)).

Las especificaciones enlazadas son la referencia del contrato completo. Un elemento planeado del roadmap no es una opción de configuración disponible.
