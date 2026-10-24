# Roadmap

S1 y S2 están entregados. S3, S4 y S5 siguen planeados. Este mapa muestra dependencias y límites de alcance, sin prometer fechas.

<!-- meada-graph:roadmap -->

## Qué está disponible

[S1, #259](https://github.com/ClaudioMascaro/meada/issues/259) entrega las bases del [modo servidor](server.md). [S2, #293](https://github.com/ClaudioMascaro/meada/issues/293) entrega la [consola](console.md), versiones inmutables y operaciones de la flota.

## Bases en construcción

[S3, #294](https://github.com/ClaudioMascaro/meada/issues/294) trata owners y autorización de entradas. [S4, #319](https://github.com/ClaudioMascaro/meada/issues/319) trata identidad, grants, directorio y auditoría. Partes de estas bases pueden avanzar en paralelo; el grafo de tickets determina los bloqueos reales. El mapa no presenta todo S3 como requisito previo de todo S4.

S5 depende de ambas bases, de la [publicación atómica, #316](https://github.com/ClaudioMascaro/meada/issues/316), y de la [medición y validación, #338](https://github.com/ClaudioMascaro/meada/issues/338).

## Después de las bases

[S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) introducirá [workspaces con nombre](named-workspaces.md) sobre indexación compartida. La [consolidación de contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), precede al runtime. La secuencia aceptada es contratos → catálogo elegible → primer workspace → versiones y superposición → configuración entre repositorios. Después, UI guiada y propuestas MCP avanzan en paralelo; la validación empresarial depende de ambas.

Las especificaciones enlazadas son la referencia del contrato completo. Un elemento planeado del roadmap no es una opción de configuración disponible.
