# Identidad empresarial

La automatización de identidad ([S4, #319](https://github.com/ClaudioMascaro/meada/issues/319), [ADR 0016](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0016-identity-automation.md)) está entregada. Suma al [modo servidor](server.md) y a la [consola](console.md) una capa de datos en Postgres, una auditoría, los tipos de proveedor de identidad, el grant de GitHub, las adiciones de equipos, un directorio SCIM y el overage de Entra ID. Cada función es opt-in: sin las secciones nuevas, todo proceso se comporta como antes. Las [recetas](identity-providers.md) de Entra ID, Okta y Keycloak muestran la configuración del lado del proveedor.

El proveedor de identidad emite el token. El grant efectivo reúne las identidades permitidas por las claims y, cuando se configura, por los permisos de GitHub. La consulta de roles en GitHub corresponde al job de refresh, nunca a la consola, a los nodos ni a la indexación. Los owners y los roles humanos no amplían ese grant. Los workspaces con nombre, la creación de contextos derivada de los roles efectivos de repositorio (directos o por grupo) y el acceso de viewer heredado sin segunda aprobación pertenecen a [S5, #339](named-workspaces.md), todavía planeado (solo el [catálogo de repositorios](github-app.md#el-catalogo-de-repositorios) está entregado); el `role_name` de GitHub no identifica un equipo, y la autoría por UI o MCP tampoco está entregada.

<!-- meada-graph:enterprise -->

Esta página sigue el orden de un despliegue: capa de datos, auditoría, tipo de proveedor, grant de GitHub, adiciones, directorio, overage y límites.

## Capa de datos

Postgres 15 o posterior guarda el estado mutable duradero: auditoría, directorio y permisos. El store sigue guardando documentos y bundles inmutables, y Valkey punteros, locks, fences y cachés. Habilita la feature `postgres` (incluida en `cloud`) y la sección `[postgres]` del manifiesto de bootstrap:

```toml
[postgres]
url_env = "MEADA_POSTGRES_URL"      # el valor por defecto; o url_file = "/run/secrets/postgres-url"
ca_file = "/etc/meada/ca/ca.pem"    # opcional: verifica solo contra este archivo
schema = "meada"                    # por defecto; minúsculas, dígitos y _, de 1 a 63 caracteres
pool_size = 4                       # por defecto
connect_timeout_seconds = 5         # por defecto
statement_timeout_seconds = 10      # por defecto
```

- **La URL** sale de `url_env` o de `url_file` (como mucho uno), nunca del manifiesto, que sigue sin secretos. Es `postgres://` o `postgresql://` con host TCP, y puede llevar usuario, contraseña, puerto y base, pero ningún parámetro de query: `sslmode` y los demás son errores, porque el TLS lo decide Meada. Un error nombra la clave, nunca la URL.
- **TLS.** Un host fuera de loopback (`localhost`, `127.0.0.0/8`, `::1`) siempre usa TLS con el nombre del host verificado; solo el loopback usa TCP simple (salvo que `ca_file` esté definido). Sin `ca_file` valen las raíces del sistema; con él, solo las del archivo.
- **El schema** `[postgres] schema` contiene todas las tablas, y Meada no escribe fuera de él; varios despliegues pueden compartir una base con schemas distintos.
- **Migraciones.** Solo `meada postgres migrate --workspace <manifiesto de bootstrap>` crea o altera una tabla. Crea el schema si falta, aplica cada migración en su transacción bajo un advisory lock del schema (dos ejecuciones se serializan) e imprime la versión alcanzada. Ejecútalo **antes de cada actualización del binario**. Todo otro comando que conecta lee la versión: una menor que la del binario es el error que nombra `meada postgres migrate`, una mayor nombra las dos versiones. La consola y los comandos del CLI se niegan a arrancar o salen con 1; un nodo escribe el error una vez en stderr y trata Postgres como inalcanzable, sin cambiar su disponibilidad.
- **Tres roles de Postgres** se recomiendan: `migrate` (dueño del schema), uno que escribe (la consola, la verificación de roles del job y los gemelos del CLI) y uno que lee (los nodos). Ejemplo, con la base `meada` y los roles ya creados:

    ```sql
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT USAGE ON SCHEMAS TO meada_write, meada_read;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO meada_write;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT USAGE, SELECT ON SEQUENCES TO meada_write;
    ALTER DEFAULT PRIVILEGES FOR ROLE meada_migrate
      GRANT SELECT ON TABLES TO meada_read;
    ```

- **Quién conecta:** la consola (auditoría, directorio, permisos, overage), los nodos con `[oidc.github]` u overage por el directorio (solo lectura), `meada github-app roles` y los gemelos del CLI que escriben. `meada index` nunca conecta.
- **Los respaldos y la disponibilidad son del operador**, como en Valkey y el store. Perder la base cuesta la única copia de la auditoría (stderr sigue); el directorio vuelve con el próximo ciclo completo del proveedor de identidad, y los permisos, con el próximo refresh.
- **Ventana de actualización.** Desde `meada postgres migrate` hasta que todo proceso ejecute el binario nuevo, los procesos antiguos pierden la capa de datos: las escrituras fallan en la auditoría y los nodos dejan envejecer el grant de GitHub.
- `meada status --workspace` con `[postgres]` imprime `postgres: reachable, schema <nombre> version <n>` o `postgres: unreachable (<motivo>)`, sin cambiar el código de salida.

El [ejemplo de Compose](https://github.com/ClaudioMascaro/meada/tree/main/deploy/compose) y el [chart de Helm](https://github.com/ClaudioMascaro/meada/tree/main/deploy/helm/meada) traen un Postgres y el paso de migración listos, desactivados por defecto.

## Auditoría

Con `[postgres]`, cada escritura de la consola y cada gemelo del CLI deja un registro: `workspace.save`, `workspace.import`, `workspace.apply`, `pointer.set`, `pointer.retain`, `pointer.drop` y `fence.recover` (más `pointer`, solo para un rechazo de rol antes de leer el cuerpo, con argumentos `{}`). El registro lleva `id`, `time` (el reloj de Postgres), `via` (`console` o `cli`), `subject`, `login` cuando hay, `role` (`viewer`, `operator` o `team_operator`), los `owners` de un operador de equipo, `action`, `arguments`, `outcome` (`pending`, `accepted`, `refused` o `failed`) y `message`. Los rechazos también se registran. El publish del job, `meada github-app roles` y las escrituras SCIM no se auditan: cada uno deja su propio registro.

La auditoría **falla cerrada**: el registro `pending` se escribe antes de que la escritura toque Valkey o el store, y, si no puede escribirse, la escritura se rechaza y nada cambia (la consola responde 503 `audit_unavailable`; el CLI sale con 1 nombrando Postgres). Una caída de Postgres detiene las escrituras, nunca las lecturas. El operador global lee los registros en la página **Registro de auditoría** de la consola o en `GET /api/v1/audit?before=<id>&limit=<1 a 100>`. La línea de auditoría en stderr no cambia. Meada nunca borra un registro: la poda es del operador, en SQL, por ejemplo `DELETE FROM meada.audit WHERE time < now() - interval '1 year'`. Más en [Consola](console.md#auditoria).

## Proveedor de identidad

`[oidc]` es el proveedor de identidad, el único emisor de tokens del despliegue; todas sus claves de hoy siguen igual. `type` dice cuál es: `generic` (por defecto), `entra`, `okta` o `keycloak`. `okta` y `keycloak` se comportan como `generic` y solo nombran la [receta](identity-providers.md); `entra` tiene la tabla `[oidc.entra]` con una clave, `overage` (`ignore`, por defecto, o `directory`; véase [Overage de Entra](#overage-de-entra)). `[oidc.entra]` bajo otro `type` es un error. Véase [Autenticación y grants](admission.md#configurar-oidc).

## Grant de GitHub

`[oidc.github]` convierte los roles de GitHub en una segunda fuente del grant, unida al de las claims: lo que el login de GitHub de un sujeto lee en GitHub lo lee en Meada.

```toml
[oidc.github]
host = "github.com"
login_claim = "github_login"       # la claim del token con el login de GitHub
roles = ["read", "triage", "write", "maintain", "admin"]   # el valor por defecto; roles personalizados también
max_age_seconds = 5400             # obligatorio, al menos 60
```

- **El login** sale de la claim `login_claim` (cada proveedor de identidad la mapea; véanse las [recetas](identity-providers.md)). Solo vale una string de 1 a 39 letras ASCII, dígitos, `-` y `_`, comparada en minúsculas. Una claim ausente o malformada no da grant de GitHub, y el grant de las claims sigue. Quien administra el proveedor de identidad controla ese mapeo: usa un atributo verificado, nunca uno que el usuario edite.
- **Los roles aceptados** son el `role_name` de GitHub, comparado exactamente, con mayúsculas y espacios; los roles personalizados de repositorio entran en la lista. Por defecto son los cinco roles nativos.
- **`max_age_seconds`** es el "intervalo de un refresh": la programación del job más su ejecución más larga. Los procesos honran el conjunto de permisos más nuevo durante ese tiempo después de su `finished`, en el reloj de Postgres; después, el grant de GitHub queda vacío, ya sea porque el job se detuvo o porque Postgres quedó inalcanzable.
- **El refresh del job** pasa a ser, bajo `meada snapshot lock`: `meada workspace pull`, `meada github-app roles`, el fetch de los espejos que lista `meada workspace show --pulled` (un script de fetch lee esa salida), `meada index --workspace` y `meada snapshot publish`.

    ```sh
    meada snapshot lock --workspace job.workspace.toml -- sh -ec '
      meada workspace pull --workspace job.workspace.toml
      meada github-app roles --workspace job.workspace.toml
      meada workspace show --pulled --workspace job.workspace.toml > pulled.toml
      sh fetch-pulled.sh pulled.toml
      meada index --workspace job.workspace.toml
      meada snapshot publish --workspace job.workspace.toml'
    ```

- **Una verificación de roles fallida detiene el refresh.** `roles` sale con 1 sin escribir ningún conjunto (todas las instalaciones limitadas, un 5xx, un timeout, Postgres inalcanzable), el conjunto anterior permanece y envejece hasta que los nodos lo rechazan, y no se publica ninguna generación con roles que el job no pudo leer. También elimina el archivo de exclusiones, así que `index`, `status` y `snapshot publish` rechazan hasta que una verificación tenga éxito: la recuperación es volver a ejecutar `meada github-app roles` (una ejecución que un fence más nuevo rechaza deja el archivo como está). Un repositorio que GitHub no muestra (404, o ninguna instalación cubre el owner) no da filas y deja una advertencia, y el refresh sigue. No es un fallo de fetch según el [#316](https://github.com/ClaudioMascaro/meada/issues/316): una exclusión por elegibilidad es una decisión previa al fetch.
- **La sesión** muestra `login`, `github_host` y `github_grant` (`current`, `expired` o `unread`), y la verificación de token separa las identidades en `sources`.
- La medición contra una organización real, con equipos y rol base, es el [#338](https://github.com/ClaudioMascaro/meada/issues/338); hasta entonces, el costo por refresh está en los [límites conocidos](#limites-conocidos).

Detalles en [Credenciales de GitHub App](github-app.md#verificacion-de-roles).

## Adiciones bajo el proveedor

Bajo `[oidc.github]`, un operador de equipo puede añadir, para su equipo, una entrada de un repositorio del `host` que su grant todavía no lee, sin mapeo en `[oidc.groups]`.

- **`added_by`** es una clave opcional de la entrada: el login de GitHub de quien la añadió (de 1 a 39 minúsculas, dígitos, `-` y `_`). Entra en el id de la versión y es un error en una entrada del propio manifiesto de bootstrap.
- **Quién la define.** El guardado de un operador de equipo define el `added_by` de cada adición en el `host` como el login de quien guarda, sea cual sea el texto enviado, y lo limpia en las demás. El operador global y el CLI guardan el valor tal como se escribe: así el operador global adopta la adición de un equipo (define o reemplaza su `added_by`, o lo limpia); sus propias adiciones no llevan `added_by` y se buscan como antes.
- **Rechazos.** `unattributed`: una adición en el `host` sin `added_by`, o de un token sin login. `added_by_change`: una edición que cambia, limpia o define el `added_by` de una entrada. Se suman a `entries_refused` en el orden `outside_grant`, `unattributed`, `ownerless`, `owner_change`, `added_by_change`, `not_your_team`.
- **El job** busca, indexa y publica la entrada solo mientras su `added_by` tiene un rol aceptado. La entrada excluida queda ausente del workspace; la consola la marca `excluded: true` en las vistas del workspace, para quien la ve, y el stderr del job nombra la identidad y el motivo, nunca el login.
- **`outside_grant` revela.** Una identidad que el workspace ya tiene fuera del grant del equipo se rechaza con `outside_grant` al guardar. Eso le muestra al equipo que existe, y el editor no puede prever ese rechazo (véanse los [límites](#limites-conocidos)).

Más en [Consola, Equipos](console.md#equipos).

## Directorio

`[directory]` (feature `directory`, incluida en `cloud`; exige `[postgres]` y `[oidc]`) activa un endpoint SCIM 2.0 en la consola, alimentado por el proveedor de identidad:

```toml
[directory]
token_env = "MEADA_SCIM_TOKEN"   # o token_file; la consola lo lee al arrancar
user_claim = "oid"               # la claim del token que identifica al usuario
user_attribute = "externalId"    # o "userName"
group_attribute = "externalId"   # "displayName" por defecto
```

- **La URL SCIM** es `<URL pública de la consola>/scim/v2/`, con `Authorization: Bearer <token SCIM>`. El token tiene al menos 32 caracteres; la consola no arranca con un token corto, ausente o ilegible, y **rotarlo exige reiniciarla**. Ningún token de la consola sirve ahí, el token SCIM no sirve en `/api/v1/`, y una petición con `Origin` recibe 403.
- **Desaprovisionamiento solo en la consola.** Un usuario cuyo `user_claim` encuentra un usuario inactivo, o eliminado y no aprovisionado de nuevo con `active` verdadero, recibe 403 `deprovisioned` en toda ruta salvo `GET /api/v1/config`. La eliminación SCIM de un sujeto aprovisionado deja un bloqueo duradero; un sujeto nunca aprovisionado sigue la admisión normal. Los nodos no verifican esto: el token vivo de un usuario desaprovisionado lee hasta expirar.

<!-- meada-graph:scim-revocation -->

- **El selector de owners.** `GET /api/v1/directory/groups?prefix=<texto>` (solo el operador global) lista hasta 100 grupos, y el editor los ofrece al editar un `owner`. Una vez aprovisionado algún grupo, una entrada cuyo `owner` no nombra ningún grupo aparece con `owner_missing: true`.

Más en [Consola, Directorio](console.md#directorio-scim).

## Overage de Entra

Un usuario de Entra ID en más grupos de los que un token puede llevar recibe `_claim_names` en lugar de `groups`. Con `type = "entra"`, `[oidc.entra] overage = "directory"` da a ese token los grupos que el directorio conoce: los `group_attribute` de los grupos de los que es miembro el usuario activo (el `user_attribute` igual al `user_claim`). Por eso exige `[directory]` y `groups_claim`: Meada nunca llama al endpoint de la fuente (Microsoft Graph), y solo el directorio que el proveedor empuja puede responder. Los grupos reemplazan la claim ausente en todo lugar donde se lee (grant, operadores, equipos, sesión). El nodo o la consola consulta el token una vez y guarda la respuesta durante `OVERAGE_TTL`. Con `overage = "ignore"`, el valor por defecto, un token en overage no tiene grupos.

## Límites conocidos

- **Costo de GitHub por refresh:** un listado por repositorio del host, `ceil(colaboradores / 100)` peticiones cada uno. Un token de instalación recibe 5.000 peticiones por hora, más 50 por repositorio o usuario por encima de 20, como máximo 12.500 (15.000 en GitHub Enterprise Cloud), y 900 puntos por minuto de REST en los límites secundarios. Con un rol base de la organización, todo miembro es colaborador de todo repositorio: 1.000 repositorios y 2.000 miembros dan 20.000 peticiones por refresh, más allá de la hora de una instalación. Repartir entre instalaciones solo funciona entre owners distintos. Pasado el presupuesto, un listado queda limitado, `roles` sale con 1 y ningún refresh publica hasta que vuelva el presupuesto, para todos los equipos. El camino de evolución, dimensionado por la medición del #338: cuando el rol base de la organización se acepta, listar sus miembros una vez y, por repositorio, solo los colaboradores que no vienen del rol base.
- **Latencia de revocación:** una revocación en GitHub llega a Meada en el próximo refresh exitoso más, como máximo, `PERMISSIONS_POLL` (60 segundos); sin un refresh exitoso, como máximo `max_age_seconds` después del último.
- **Memoria:** un proceso guarda todas las filas de rol aceptado del conjunto más nuevo (logins por repositorios, bajo un rol base).
- **`OVERAGE_TTL`** (5 minutos): un cambio de miembros llega a un sujeto en overage con ese retraso.
- **Token vivo:** el token de un usuario desaprovisionado sigue leyendo en los nodos hasta expirar.
- **`outside_grant`** le dice a un operador de equipo que la identidad ya está en el workspace, y el editor no lo prevé.
- **Un host de GitHub** por despliegue; un login tiene como máximo 39 caracteres y un nombre de rol, 100.
- **SCIM:** el token se lee al arrancar (rotarlo exige reiniciar la consola); cuerpos de hasta `SCIM_BODY_MAX` (1 MiB), un recurso almacenado de hasta `SCIM_RESOURCE_MAX` (64 KiB) y páginas de hasta 100.
- **La auditoría crece sin límite** hasta que el operador la pode.
- **Postgres 15 o posterior.** Desde `meada postgres migrate` hasta que todo proceso ejecute el binario nuevo, los procesos antiguos pierden la capa de datos: las escrituras fallan en la auditoría y los nodos dejan envejecer el grant de GitHub.
