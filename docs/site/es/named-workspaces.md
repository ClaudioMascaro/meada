# Workspaces con nombre

!!! success "Entregado: S5, validado de extremo a extremo"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) está entregado en ocho porciones ([#340](https://github.com/ClaudioMascaro/meada/issues/340) a [#347](https://github.com/ClaudioMascaro/meada/issues/347)): los contratos (ADR 0020 a 0022), el [catálogo](github-app.md#el-catalogo-de-repositorios), el primer workspace, el ciclo de vida de las versiones, la configuración por workspace, la autoría guiada, las propuestas MCP y esta validación. Todo lo siguiente se construye detrás de la feature de cargo `workspaces` (y `proposals` para el servidor de propuestas), ambas habilitadas por `cloud`. Sin `[workspaces]` la consola sigue operando con un workspace por deployment. Una verificación sigue abierta: [#338](https://github.com/ClaudioMascaro/meada/issues/338), la medición contra una organización real de GitHub, que el mantenedor decidió que no condiciona S5 ([evidencia de descubrimiento](#evidencia-de-descubrimiento)).

## Componer sin duplicar la indexación

Los workspaces con nombre se superponen sobre indexación compartida. Un repositorio no obtiene otro índice, job, bloqueo de refresh ni generación por pertenecer a dos workspaces: un único refresh obtiene, indexa y publica una vez cada repositorio seleccionado, y cada workspace es un contexto que selecciona algunos de ellos. Hay tres roles humanos: administrador de la plataforma, operador de workspace y viewer.

El grant efectivo por repositorio sigue siendo la autoridad. Con Y legible por los grupos A y B, el repositorio A solo por el grupo A y el repositorio B solo por el grupo B, un operador solo en A puede componer Y+A y se le rechaza Y+B; un operador solo en B puede componer Y+B y se le rechaza Y+A; quien está en ambos puede componer los tres. Compartir Y no concede A ni B a nadie, y un enlace de Y a B no aparece en el workspace de Y+A.

<!-- meada-graph:workspace-overlap -->

## El primer workspace

Un mantenedor admitido, sin ninguna entrada propia, abre **Workspaces con nombre** en la consola, elige repositorios del catálogo, da nombre al workspace y confirma el manifiesto que muestra la página. El servidor decide de nuevo en una transacción contra el conjunto de permisos más nuevo: todo repositorio seleccionado debe estar en el catálogo y el login de GitHub de la persona debe tener un rol aceptado en él, directo o por un grupo. Un rechazo nombra los repositorios que la concesión de la persona lee y solo cuenta los demás. El workspace se guarda a nombre de la persona, que lo opera; los colegas cuyas concesiones propias leen los mismos repositorios lo ven sin invitación alguna y no pueden modificarlo.

El workspace pasa a poder consultarse por el job compartido: `meada workspace pull` lo lee, `meada github-app roles` deja fuera, antes del fetch, toda entrada en la que ninguno de sus operadores tiene un rol aceptado (una decisión de política, no un fetch fallido) y los repositorios que solo seleccionan workspaces con nombre se descargan, indexan y publican una vez, en `<[workspaces] mirrors>/<identity>`. Hasta que una generación registra el workspace, la consola lo muestra como `applied` con sus entradas `pending`; una vez registrado es `live` y sus entradas `ready`. Un fetch o una indexación fallidos siguen abortando toda la publicación y manteniendo la generación actual.

## Mantener un workspace

Un operador edita su workspace como una secuencia de versiones. **Guardar** almacena una versión y no mueve nada; **aplicar** hace de una versión guardada la actual, y aplicar una que ya fue la actual es un **rollback**. Todo cambio lleva la **base** que leyó la página (el número que muestra `GET /api/v1/workspaces/<id>`): si alguien cambió el workspace desde entonces, el cambio se rechaza (`stale_base`, indicando la base actual) y no se escribe nada, de modo que dos personas editando a la vez nunca se pisan. En el momento de escribir, el servidor comprueba de nuevo, en una transacción, que el workspace sigue existiendo, que la base es la actual, que la persona aún lo opera (o es el administrador de la plataforma) y que todo repositorio que el cambio **añade o altera** sigue siendo elegible para ella en el conjunto de permisos más nuevo; los repositorios que una versión mantiene sin cambios y las eliminaciones no se vuelven a comprobar. Un rol revocado, una entrada rechazada, una página vieja o una versión de otro workspace no cambian nada.

Un operador que perdió el rol en un repositorio conserva el workspace, lee las entradas que su concesión lee y un recuento de las demás, y aún puede editarlas: la versión que guarda mantiene en su sitio, en orden, las entradas que no lee, y no puede leerlas ni sobrescribirlas. Elimina una volviendo a una versión que no la tiene, pidiéndoselo al administrador de la plataforma (cuya vista de gestión no se filtra) o borrando el workspace. Los colegas que perdieron el rol en cualquiera de los repositorios dejan de ver el workspace, sus versiones y su historial: reciben el 404 de un id que no existe. Un lector, es decir, quien lee el workspace sin operarlo, no escribe: recibe un 403 `not_operator`; y leer un repositorio nunca edita el workspace de otra persona.

**Renombrar** cambia solo el nombre y la descripción (el id sigue igual). **Borrar** quita el workspace de las listas y del siguiente refresh, conservando sus versiones y su historial en Postgres; los repositorios que seleccionaba siguen indexados una sola vez para cada otro workspace que los seleccione, y las generaciones retenidas conservan el contexto hasta que se descartan. Solo el administrador de la plataforma **delega** operadores (un workspace conserva al menos uno). Los workspaces superpuestos sobre un mismo repositorio son independientes: editar, revertir o borrar uno no mueve nada en el otro. Cada operación, rechazos incluidos, es un registro de auditoría con la persona, el workspace y la base; `meada workspaces list|show|versions|diff|create|save|apply|rename|delete|operators` hacen lo mismo desde la línea de comandos, como administrador de la plataforma.

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

Cada entrada del manifiesto de un workspace puede llevar, además de `repo`, bloques `[repository.serve]` (`hosts`, `prefix`, `strip`), `[[repository.resource]]` y `[[repository.peer]]` con las claves de `meada.toml`. El `base_url_env` de un peer lista **nombres** de variables de entorno, nunca un valor, una URL ni `NOMBRE=valor`; Meada no las lee. `path`, `owner`, `added_by` y `[repository.serve] documents` se rechazan (422), porque las rutas de mirror y las credenciales son de la plataforma y los documentos deciden qué se indexa. Las capas nunca se mezclan: el `meada.toml` del propio repositorio gana por completo; si no, valen los bloques del workspace; si no, el repositorio no declara nada.

**Antes de guardar**, el editor de la consola (y `POST /api/v1/workspaces/effective`, que no guarda nada) muestra la configuración efectiva del texto; **después**, `GET /api/v1/workspaces/<id>/effective[?version=]` la muestra para una versión guardada. Por entrada dice la `source` (`repository`, `context` o `none`), los bloques `ignored`, la `coverage` (`indexed`, `pending_ingestion` o `excluded`) y diagnósticos: `ignored_by_repository`, `unknown_until_ingested` (el repositorio no está en ninguna generación mantenida, así que aún no se sabe si su archivo lo sustituye), `excluded_no_sponsor`, `peer_missing` (un peer fuera del workspace se señala, nunca se descubre) y `documents_not_indexed` (hosts de serve para un repositorio cuya ingesta no declara `[serve]`). Un guardado, una aplicación y una creación llevan los mismos diagnósticos junto al resultado, así que una declaración ignorada nunca aparece como simplemente aplicada. La vista la calcula la misma función que ejecuta el navegador (WebAssembly) y nunca entra en el texto del manifiesto ni en ningún id.

Un cambio de configuración es una nueva generación de los mismos bundles: nada se reindexa, una generación fijada antes del cambio conserva la configuración que registró, y otro workspace que comparte el repositorio no se mueve. La `base` del workspace se verifica como en cualquier cambio.

## Autoría guiada

La página de workspaces de la consola edita un workspace que operas mediante **controles de formulario** y mediante el **texto del manifiesto**, y los dos son un solo documento: una edición en cualquiera reescribe el otro, ambos pasan por el mismo WebAssembly del servidor (`meada-core`), y lo que se guarda es el texto canónico que escribe. El formulario no tiene regla propia: lo que una entrada puede contener, qué repositorios puedes seleccionar, el diff y las entradas que un cambio debe encontrar elegibles de nuevo son del core, y el servidor decide otra vez en la transacción. Si el texto tiene errores, el formulario queda de solo lectura hasta que se corrijan.

- **Repositorios**: se agregan desde el catálogo (búsqueda por prefijo, página a página) y se quitan del manifiesto. Un repositorio que el catálogo no listó debe verificarse antes (`POST /api/v1/catalog/check`, que solo se hace al pulsar *Verificar elegibilidad*; las demás peticiones del formulario son las páginas del catálogo y *Mostrar la configuración efectiva*, y el resto se calcula localmente), y cada entrada cambiada dice en texto si está *ya en el workspace* (se verifica de nuevo solo si la cambias), *seleccionable*, *no disponible*, *con verificación pendiente* o *aún no verificada*; un cambio que toca un repositorio para el que no eres elegible no se puede guardar y nombra los que tu grant lee y cuenta los demás.
- **Ajustes de serve** (`hosts`, `prefix`, `strip`), **recursos** (`kind`, `id`, `role`, `via`) y **peers** (el repositorio del peer y los **nombres** de las variables de entorno con la URL base, nunca un valor) son campos de cada repositorio. Un campo vaciado es un campo ausente, y un valor que el core rechaza se muestra como el mensaje del core.
- **Alcance y configuración efectiva** se muestran en texto, nunca solo por color: si operas el workspace, cuántas entradas editas y cuántas no puedes leer (se mantienen como están al guardar), la ingesta de cada entrada (`pending`, `ready`, `excluded` o nueva) y, tras *Mostrar la configuración efectiva*, qué origen gana y qué ignora el `meada.toml` del propio repositorio.
- **Revisar y guardar.** La página lista el diff (entradas agregadas, quitadas y cambiadas, con las partes cambiadas) y *Guardar y aplicar* abre una confirmación que **congela** el contenido revisado: el texto del manifiesto, el mensaje y la base. No se envía nada más al confirmar, pase lo que pase con el borrador. Un rechazo se muestra como lo redacta el servidor: `entries_refused` nombra los repositorios (y cuenta los ocultos) y lee de nuevo tus roles para que la elegibilidad mostrada sea la actual, `stale_base` ofrece recargar el workspace (una versión guardada cuya aplicación fue rechazada se dice como tal), y perder el rol de operador retira el editor. Quien solo ve el workspace ve las entradas y su estado, pero ningún control que cambie algo.

Calcular estas reglas locales no abre ninguna conexión. Los dos idiomas de la consola (inglés y portugués de Brasil) tienen los mismos controles. No construido: una página de las versiones de un workspace con reversión (las rutas `versions` e `history` existen); la interfaz de propuestas es la [autoría asistida por MCP](#autoria-asistida-por-mcp), a continuación.

<!-- meada-graph:mcp-proposal -->

## Autoría asistida por MCP

Un **servidor de propuestas** separado (feature `proposals`, [ADR 0022](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0022-authenticated-workspace-proposals.md)) permite que un agente de CLI, actuando por un operador, liste los repositorios que ese operador puede seleccionar y pregunte si un manifiesto es válido y está autorizado. Deriva la identidad, el login, los grants, el rol y los workspaces que el operador opera de su propia autenticación, nunca de argumentos; no reenvía ninguna credencial y trata todo lo que el agente envía como no confiable. Devuelve el manifiesto canónico, un diff, diagnósticos y la base, y no escribe nada: ningún borrador, ninguna versión, ningún puntero movido, ningún registro de auditoría. Una persona pega la propuesta en la consola, que la valida de nuevo y vuelve a comprobar la base, la autoridad y los grants al guardar y aplicar.

### Ejecutar el servidor de propuestas

Agregue `[proposals]` al manifiesto de bootstrap que leen la consola y el job (requiere `[workspaces]`):

```toml
[proposals]
public_url = "https://proposals.example.com"  # https, salvo en loopback, sin ruta
# audience = "https://proposals.example.com"  # el aud de sus tokens, public_url por defecto
```

Ejecute `meada workspaces mcp --listen 0.0.0.0:8090 --workspace meada.workspace.toml` en un proceso propio, junto a la consola y nunca en un nodo. Lee lo que lee la consola: Postgres (toda sesión de solo lectura), los documentos de versión del store, las generaciones que mantiene su `[snapshot] local_dir` (mantenga un `meada snapshot pull` en marcha para él como para la consola, o la configuración efectiva muestra todo repositorio como pendiente) y los metadatos y las claves del emisor. Nunca llama a GitHub, a un nodo ni a la API del proveedor de identidad, y no guarda clave de GitHub App. El endpoint MCP es la raíz de `public_url`, y los metadatos de recurso protegido están en `/.well-known/oauth-protected-resource`. Registre `[proposals] audience` como recurso en el proveedor de identidad: un token de nodo o de la consola se rechaza aquí (401), y el token de este servidor lo rechazan ellos.

| Herramienta | Qué responde |
| --- | --- |
| `catalog` | Los repositorios que quien llama puede seleccionar (`query`, `limit`, `after`), como el catálogo de la consola: `state`, `reason`, la edad del conjunto y una página de identidades. |
| `workspaces` | Los workspaces que quien llama puede ver, con `seq` (la base) y si quien llama opera cada uno. |
| `show` | Un workspace visible: su registro, `seq`, las entradas que quien llama lee y su texto de manifiesto. |
| `propose` | `entries` (texto de manifiesto), `workspace` (el workspace a cambiar, si lo hay), `name`, `description` y `hints`: la propuesta, a continuación. |

Una propuesta es `{base, manifest, diff, diagnostics, eligibility, refusals, effective, applicable}`. `base` es `{workspace, seq, version}`, nula para un workspace nuevo: lo que el guardado de la consola todavía debe encontrar. `manifest` es el texto canónico de las entradas que quien llama lee, sin anotación de vista; `diff` lo compara con las entradas de la base que quien llama lee; `diagnostics` son los problemas de validación en las palabras de la CLI, con línea, entrada y clave cuando los nombran; `eligibility` da cada identidad nombrada como `selectable`, `unavailable` o `pending`; `refusals` son los errores que la consola respondería ahora (`not_operator`; `entries_refused`, que nombra solo lo que quien llama lee y cuenta el resto en `hidden`; `pending` con su motivo); `effective` es la configuración efectiva sobre la base; `applicable` dice si la consola la aceptaría ahora. Toda herramienta es de solo lectura, y ninguna tiene un argumento que nombre actor, rol, grant o login: uno así se rechaza como argumento inválido. Las entradas están acotadas (cuerpo de 80 KiB, manifiesto de 64 KiB, 50 hints con notas de 256 caracteres, páginas de 100), y una respuesta de más de 192 KiB es `too_large`, nunca recortada.

### La sesión de un agente

1. Configure dos servidores MCP en el agente: el MCP de lectura de GitHub que su organización ya autoriza, con sus propias credenciales, y el servidor de propuestas de Meada, con un token para `[proposals] audience` obtenido de su proveedor de identidad. Meada nunca recibe las credenciales de GitHub, y el MCP de GitHub nunca recibe el token de Meada.
2. Pregunte a `catalog` qué puede seleccionar el operador, y a `workspaces` y `show` qué existe ya.
3. Lea los repositorios candidatos con el MCP de GitHub: rutas, clientes, paquetes, qué repositorio sirve qué contrato. Lo que diga sobre acceso es una pista, no un grant.
4. Llame a `propose` con el texto del manifiesto, el `workspace` a cambiar, si lo hay, y los repositorios leídos como `hints`. Una pista nunca agrega una entrada ni cambia un resultado, y su nota se descarta sin leerse.
5. Corrija lo que señalan `diagnostics` y `refusals` y proponga de nuevo hasta que `applicable` sea verdadero. Llamadas iguales dan respuestas iguales.
6. Entregue el `manifest` y la `base` al operador para su revisión en la consola. Para un cambio, el operador pega el texto en el editor del workspace, revisa la configuración efectiva y el diff, y guarda y aplica en esa base. Para un workspace nuevo, el operador lo crea desde el catálogo con los mismos repositorios y luego pega el texto en su editor. Si alguien cambió el workspace entretanto, la consola rechaza la base vieja y el agente propone de nuevo.

La distinción entre [versión de workspace y generación](workspaces.md), y entre [admisión y grant](admission.md), sigue siendo esencial para este diseño.

## Desplegar y actualizar

El orden importa, porque los nodos verifican lo que descargan.

1. **Haga una copia de seguridad de Postgres y luego migre.** `meada postgres migrate --workspace <bootstrap>` lleva el esquema a la versión 8 (6: el `source` de las filas del catálogo; 7: las cuatro tablas de workspace y el contexto de las exclusiones; 8: las identidades de cada versión). Un proceso todavía en una versión anterior rechaza el esquema nuevo, como dice [la ventana de actualización de la capa de datos](enterprise.md#limites-conocidos). Las migraciones solo avanzan: para volver atrás, restaure la copia.
2. **Actualice todos los binarios, nodos incluidos, antes del primer refresh que registre un contexto.** Un nodo recalcula el id de la generación que descarga; un nodo más antiguo rechaza una generación cuyo id también hashea los nuevos `members` y `contexts` (y, desde #510, las identidades `excluded` de un contexto, así que actualice los nodos antes del publicador o junto con él otra vez en esa versión), conserva su última generación y su disponibilidad, y nunca sirve el repositorio de un workspace con nombre en ninguna ruta (falla cerrado). Una generación sin workspace con nombre la leen todos los nodos.
3. **Configure el job y la consola** con `[catalog]` (`allow`, `max_repositories`) y `[workspaces]` (`mirrors`), que requieren `[ui]` con `operations = true`, `[oidc.github]` y `[workspace] source = "store"`. Provisione la raíz de `mirrors` donde escribe el paso de fetch del job: `meada workspace show --pulled` lista cada miembro y su ruta. Los ejemplos de [Compose](https://github.com/ClaudioMascaro/meada/tree/main/deploy/compose) y [Helm](https://github.com/ClaudioMascaro/meada/tree/main/deploy/helm/meada) incluyen estas secciones.
4. **Ejecute un refresh** (`workspace pull`, `github-app roles`, fetch, `index`, `snapshot publish`). El comando de roles escribe el catálogo en el mismo conjunto que los roles; la consola muestra el catálogo como `ready` cuando lo lee.
5. **Deje que las personas creen workspaces.** Los nodos sirven un contexto desde la primera generación que lo registra. Opcionalmente, ejecute el [servidor de propuestas](#ejecutar-el-servidor-de-propuestas) en un proceso propio.
6. **Apagarlo.** Quitar `[workspaces]` hace que las nuevas generaciones dejen de registrar contextos; las generaciones retenidas conservan los suyos para los binarios que los entienden, y las tablas permanecen.

### Cuando falla un refresh

| Etapa que falla | Lo que conserva el deployment | Recuperación |
| --- | --- | --- |
| `github-app roles` (5xx de GitHub, throttle, Postgres) | El conjunto de permisos anterior decide y envejece; no se escribe ningún conjunto; el puntero no se mueve. Un grant que GitHub ya muestra restaurado no vuelve. | Ejecute `roles` de nuevo; solo un conjunto completo posterior cambia un grant. |
| fetch o `index` de cualquier miembro | Todo workspace sigue respondiendo desde la última generación completa. Una revocación que el job confirmó antes sigue vigente. | Arregle el espejo y ejecute el refresh. Una entrada excluida por falta de patrocinador es una decisión previa al fetch, no un fallo, y no necesita espejo. |
| `snapshot publish` (publicación local) | El puntero no se mueve; nada se publica parcialmente. | Reconstruya el índice y publique de nuevo. |

## Validado de extremo a extremo

El flujo se acepta mediante pruebas que accionan los binarios y transportes públicos (la consola, los comandos del job, un nodo y el servidor de propuestas sobre el emisor de loopback, una API REST falsa de GitHub y servicios locales de Valkey, Postgres y compatibles con S3) y mediante la misma tabla Y/A/B en Rust y WebAssembly. Se ejecutan en CI con los servicios requeridos.

| Qué se muestra | Dónde |
| --- | --- |
| Administrador de la plataforma, un operador de A creando Y+A, un operador de B creando Y+B, un llamador en ambos, roles directos y por grupo, peers que ven sin inscripción, sin operar nada | `tests/it/ui/oidc/workspaces.rs` (`creation_follows_the_shared_table...`, `peers_view_a_workspace...`, `a_created_workspace_becomes_queryable...`), `meada-core/tests/named_workspaces.rs`, `ui/tests/named-core.test.ts` |
| Revocación tras la creación, privacidad del lector parcial, sin ampliación por Y | `workspaces/lifecycle.rs` (`a_partial_revocation_hides...`), `catalog.rs` |
| Ciclo de vida superpuesto | `workspaces/lifecycle.rs` (`overlapping_workspaces_live_and_end_independently...`, `concurrent_edits_of_one_base...`), `workspaces/configuration.rs` |
| SCIM desactiva, borra y reprovisiona; limpiar el owner es `owner_change` | `deprovision.rs`, `directory.rs`, `workspaces/lifecycle.rs` (`a_deleted_user_is_blocked...`), `workspaces/proposals.rs` (`a_deleted_user_is_refused...`) |
| UI guiada y propuestas sin estado con resultados iguales; solo un guardado deliberado cambia el estado, y queda auditado | `workspaces/proposals.rs` (`proposals_follow_the_shared_table...`, `a_proposal_changes_nothing...`), `ui/tests/GuidedWorkspace.test.tsx` |
| Fallo de roles, de fetch/index y de la publicación local | `workspaces/acceptance.rs` (`every_failed_stage_keeps_the_generation...`), `workspaces.rs` (fallo de index, exclusión por patrocinador), `github_app/roles.rs` |
| Build por defecto sin cambios | `cargo tree` de las features por defecto y los goldens de CLI, MCP, export y trace, sin cambios en todas las porciones de S5 |

## Evidencia de descubrimiento

Lo que establecen las pruebas: el catálogo y la verificación de roles contra un GitHub falso (páginas, throttling, el tope, un fallo que no escribe nada) y la fórmula de costo en [el catálogo](github-app.md#el-catalogo-de-repositorios): `ceil(covered / 100)` páginas de instalación por instalación y `ceil(collaborators / 100)` páginas de colaboradores por candidato. Lo que no pueden establecer es la cobertura de instalaciones y el costo de tasa de una organización real, y el resultado no lo sustituye el falso. [#338](https://github.com/ClaudioMascaro/meada/issues/338) sigue abierto como esa evidencia: la hipótesis que confirmaría es que el listado de instalaciones más el listado de colaboradores con `affiliation=all` cabe en la hora de una instalación con su `max_repositories`, o bien que antes hace falta listar una vez los miembros del rol base ([límites conocidos](enterprise.md#limites-conocidos)). Hasta que se mida, aumente `max_repositories` por pasos y observe si `roles` termina dentro de la programación del job.

## Orden de entrega

Contratos, después el catálogo elegible, el primer workspace, versiones y superposición, configuración por contexto, autoría guiada y propuestas por MCP, y por último esta validación. La [hoja de ruta](roadmap.md) sigue cada etapa.
