# Workspaces con nombre

!!! warning "Parcialmente entregado: S5, contratos decididos; el catálogo y el primer workspace ya existen"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) es alcance secuencial aceptado después de S3 y S4. La [consolidación de contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), está terminada: los ADR 0020, 0021 y 0022 fijan los contratos descritos aquí. Del runtime se entregaron dos porciones: el **catálogo** ([#341](https://github.com/ClaudioMascaro/meada/issues/341)), descrito en [el catálogo](github-app.md#el-catalogo-de-repositorios) y en la [consola](console.md#catalogo-de-repositorios), y el **primer workspace** ([#342](https://github.com/ClaudioMascaro/meada/issues/342)): `[workspaces]`, creación, listado y consulta, ingestión compartida con la regla del patrocinador y la ruta del nodo, descritos en la [consola](console.md#workspaces-con-nombre). Guardar versiones, revertir, renombrar, borrar y delegar, la configuración por workspace, la autoría guiada y las propuestas MCP no están construidos, y el resto de lo siguiente no es todavía una opción de configuración. Sin `[workspaces]` la consola sigue operando con un workspace por deployment.

## Componer sin duplicar la indexación

Los workspaces con nombre se superponen sobre indexación compartida. Un repositorio no obtiene otro índice, job, bloqueo de refresh ni generación por pertenecer a dos workspaces: un único refresh obtiene, indexa y publica una vez cada repositorio seleccionado, y cada workspace es un contexto que selecciona algunos de ellos. Hay tres roles humanos: administrador de la plataforma, operador de workspace y viewer.

El grant efectivo por repositorio sigue siendo la autoridad. Con Y legible por los grupos A y B, el repositorio A solo por el grupo A y el repositorio B solo por el grupo B, un operador solo en A puede componer Y+A y se le rechaza Y+B; un operador solo en B puede componer Y+B y se le rechaza Y+A; quien está en ambos puede componer los tres. Compartir Y no concede A ni B a nadie, y un enlace de Y a B no aparece en el workspace de Y+A.

<!-- meada-graph:workspace-overlap -->

## El primer workspace

Un mantenedor admitido, sin ninguna entrada propia, abre **Workspaces con nombre** en la consola, elige repositorios del catálogo, da nombre al workspace y confirma el manifiesto que muestra la página. El servidor decide de nuevo en una transacción contra el conjunto de permisos más nuevo: todo repositorio seleccionado debe estar en el catálogo y el login de GitHub de la persona debe tener un rol aceptado en él, directo o por un grupo. Un rechazo nombra los repositorios que la concesión de la persona lee y solo cuenta los demás. El workspace se guarda a nombre de la persona, que lo opera; los colegas cuyas concesiones propias leen los mismos repositorios lo ven sin invitación alguna y no pueden modificarlo.

El workspace pasa a poder consultarse por el job compartido: `meada workspace pull` lo lee, `meada github-app roles` deja fuera, antes del fetch, toda entrada en la que ninguno de sus operadores tiene un rol aceptado (una decisión de política, no un fetch fallido) y los repositorios que solo seleccionan workspaces con nombre se descargan, indexan y publican una vez, en `<[workspaces] mirrors>/<identity>`. Hasta que una generación registra el workspace, la consola lo muestra como `applied` con sus entradas `pending`; una vez registrado es `live` y sus entradas `ready`. Un fetch o una indexación fallidos siguen abortando toda la publicación y manteniendo la generación actual.

## Quién puede qué

- **Administrador de la plataforma**: el operador global que ya existe. Configura el deployment, delega operadores y administra cualquier workspace. Ningún rol serializado cambia de nombre.
- **Operador de workspace**: quien creó un workspace o recibió su delegación. Edita, aplica, revierte, renombra y elimina ese workspace y ningún otro. Estar en el mismo grupo que el creador no concede nada de esto.
- **Viewer**: nadie lo asigna. Una persona ve un workspace cuando su propio grant actual lee todos sus repositorios, así que los compañeros de grupo no necesitan invitación, grupo en el proveedor de identidad ni paquete de acceso.

Crear un workspace exige un rol aceptado de GitHub, directo o por grupo del repositorio, en todos los repositorios seleccionados. El grant por claims sigue valiendo para leer, no para crear. La elegibilidad nunca se almacena: cada decisión lee el conjunto de permisos más reciente, y un conjunto más antiguo que `max_age_seconds` hace que nada sea seleccionable hasta que el job escriba otro.

## Qué ve un lector parcial

Un workspace queda oculto para quien tiene un grant que no lee todos sus repositorios: desaparece de las listas y responde el mismo 404 que un id que no existe. Los nombres, descripciones, versiones y diagnósticos siguen la misma regla, porque un título puede revelar un repositorio que la persona no puede leer. Los operadores y el administrador de la plataforma son una excepción, para poder reparar su propio workspace: un operador que perdió el rol en A ve los repositorios que puede leer y el conteo de los ocultos, y toda respuesta del grafo sigue filtrada por el grant. Los workspaces se identifican con un id determinista y los nombres pueden repetirse, así que crear uno no revela nada sobre los ocultos.

La pérdida de un rol o de la pertenencia a un grupo quita el acceso en el siguiente conjunto de permisos y nunca revive desde uno más antiguo. El workspace, sus versiones y su historial no se borran.

## Contratos distintos

1. **Catálogo** (entregado): los repositorios que cubre el GitHub App y que admite la lista de permitidos del administrador, listados por el job en el mismo conjunto de permisos que los roles. Quien consulta ve solo los repositorios que puede seleccionar; la cobertura no es un derecho.
2. **Entradas seleccionadas**: un manifiesto versionado de repositorios con peers, relaciones de proveedor, referencias a variables de entorno, recursos y configuración de serve. No lleva ruta de mirror, valor secreto, owner ni lista de documentos.
3. **Ingestión compartida**: una entrada por repositorio entre todos los workspaces. Una entrada en la que ningún operador de su workspace tiene un rol aceptado se excluye antes del fetch, lo que no es un fetch fallido. Un fallo de fetch o de índice sigue abortando la publicación entera, y todo workspace conserva la última generación completa.
4. **Versiones**: guardar, aplicar, revertir, renombrar y eliminar, cada una comprobada en una transacción contra la versión base, la autoridad de quien la hace y la elegibilidad actual, y cada una auditada.
5. **Selección de la consulta**: `/-/workspaces/<id>/<identity>` responde desde el workspace tal como una generación lo registró, limitado por el manifiesto y por el grant. Una generación fijada lo reproduce exactamente. La ruta simple `/<identity>` sigue sirviendo el workspace del deployment.

## Configuración efectiva

El `meada.toml` del propio repositorio sigue ganando por completo sobre las declaraciones de cualquier workspace, y el editor muestra cuándo una declaración se ignora. Las declaraciones para repositorios sin él son por workspace: dos workspaces que comparten Y pueden declarar peers o hosts de serve distintos sin un segundo índice, porque se aplican al responder y nunca entran en el bundle. Qué documentos se indexan pertenece a la ingestión y es igual en todos los workspaces.

<!-- meada-graph:mcp-proposal -->

## Autoría asistida por MCP

Un servidor de propuestas separado permite que un agente de CLI, usando su propio conector de lectura de GitHub, liste los repositorios que su operador puede seleccionar y pregunte si un manifiesto es válido y está autorizado. Deriva identidad y grants de su propia autenticación, nunca de argumentos, no reenvía ninguna credencial y trata todo lo que el agente envía como no confiable. Devuelve el manifiesto canónico, un diff, diagnósticos y la versión base, y no escribe nada: ningún borrador, ninguna versión, ningún puntero movido. Una persona pega la propuesta en la consola, que la valida de nuevo y vuelve a comprobar la base, la autoridad y los grants al guardar y aplicar.

La distinción entre [versión de workspace y generación](workspaces.md), y entre [admisión y grant](admission.md), sigue siendo esencial para este diseño.

## Orden de entrega

Contratos, después el catálogo elegible, el primer workspace, versiones y superposición, y la configuración por contexto. La autoría guiada y las propuestas por MCP avanzan entonces en paralelo, y la validación empresarial depende de ambas. La [hoja de ruta](roadmap.md) sigue cada etapa.
