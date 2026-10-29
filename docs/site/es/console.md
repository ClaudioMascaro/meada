# Consola de gestión

La consola usa la feature `ui`, incluida en `cloud`, y funciona en un proceso propio. Tiene editor, diff, playground y grafo. El editor guarda versiones de configuración; aplicar una versión y publicar índices son pasos separados.

Protege el acceso antes de exponer la consola. Sin `[oidc]`, el proxy controla la admisión: quien alcance la consola fuera de esa protección podrá leer el grafo. El proxy debe conservar `Host`, `Origin` y `Authorization`. La consola acepta solo su Host público y, si hay Origin, su propio origen.

<!-- meada-graph:console-authoring -->

## Configurar el bootstrap

El bootstrap es el manifiesto que inicia la consola. Contiene los módulos y usa directorios propios, separados de los nodos. Las entradas de repositorios vienen del workspace gestionado.

```toml
index_dir = "/var/lib/meada/console/indexes"

[workspace]
source = "store"

[snapshot]
endpoint = "https://store.exemplo.internal"
region = "us-east-1"
bucket = "meada"
prefix = "production"
path_style = true
credentials = "static"
access_key_id_env = "AWS_ACCESS_KEY_ID"
secret_access_key_env = "AWS_SECRET_ACCESS_KEY"
local_dir = "/var/lib/meada/console/generations"
node_name = "meada-console-0"
# ca_file = "/run/secrets/ca.crt"

[valkey]
url = "rediss://meada@cache.exemplo.internal:6379"
key_prefix = "meada"
password_env = "MEADA_VALKEY_PASSWORD"
# ca_file = "/run/secrets/ca.crt"

[generations]
keep = 3

[refresh_lock]
ttl_seconds = 300

[oidc]
issuer = "https://id.exemplo.internal/realms/agentes"
public_url = "https://mcp.exemplo.internal"
audience = "https://mcp.exemplo.internal"
identities_claim = "meada_repositories"
groups_claim = "groups"
scopes = ["meada:read"]

[oidc.groups]
"meada-readers" = ["github.exemplo.internal/acme/api"]
"meada-operators" = ["*"]

[[github_app]]
host = "github.com"
api = "https://api.github.com"
app_id = 123456
private_key_env = "MEADA_GITHUB_APP_KEY"

[ui]
dir = "/usr/share/meada/ui"
public_url = "https://console.exemplo.internal"
client_id = "meada-console"
audience = "https://console.exemplo.internal"
operations = false
operator_groups = ["meada-operators"]
# operator_claim = "meada_operator"
```

`[oidc]` reproduce la política de los nodos, con URL pública y audience de MCP. `[ui]` define el origen y el audience de la consola. Exporta las credenciales mediante el entorno o los archivos de secretos indicados.

<details markdown="1" class="reference-details">
<summary>Certificados, GitHub App e índices de la consola</summary>

`snapshot.ca_file` añade la CA privada a las raíces del sistema. `valkey.ca_file` usa solo las raíces de ese archivo. Omite `[[github_app]]` si el job no usa Apps.

La consola lee la caché de GitHub App, pero no abre la clave privada ni llama a GitHub. Tampoco llama a los nodos: playground y exports usan los índices descargados por el pull de la propia consola.

</details>

## Iniciar la consola

[Inicializa el workspace gestionado](workspaces.md) y espera a que el job publique la primera generación. Haz el pull inicial en el `local_dir` de la consola. Ejecuta el pull periódico y el servidor en procesos separados, usando ese directorio exclusivo de la consola:

```sh
meada snapshot pull --workspace /etc/meada/console.workspace.toml
meada snapshot pull --every 30 --workspace /etc/meada/console.workspace.toml
# Run in a separate process, alongside the pull companion.
meada ui --listen 0.0.0.0:7334 --workspace /etc/meada/console.workspace.toml
```

## Login en el navegador

Registra en el issuer un cliente público, sin secreto, con Authorization Code y PKCE S256. Configura el audience y el redirect URI exacto `https://console.exemplo.internal/auth/callback`.

El navegador debe alcanzar los metadatos y el endpoint de token del issuer, con CORS para el origen de la consola. Intercambia el código directamente con el issuer. El bearer queda en memoria; `state` y `verifier` quedan en sessionStorage hasta que el callback los elimina. Recargar la página reinicia el login.

## Flota y diagnóstico

Cada `snapshot pull --every` envía un heartbeat, el registro periódico de estado del nodo. Informa generación, fence, generaciones locales, disponibilidad, versión y hora.

Usa un `snapshot.node_name` único para cada nodo y la consola, con hasta 63 letras minúsculas, dígitos, puntos o guiones. Sin él, el proceso usa un hostname válido. Los nombres de pods pueden cumplir esa función.

Un nodo sin heartbeat no aparece en la flota. Heartbeats malformados aparecen separados y no entran en el diagnóstico. Las páginas muestran flota, store, retención, commits, schemas, caché de Apps y admisión; la consola identifica las secciones ausentes.

## Roles y operaciones

Con OIDC, los lectores ven solo las identidades admitidas por su token. Para conceder el rol de operador, elige **una** opción: grupo en `ui.operator_groups` o claim en `ui.operator_claim` con valor booleano `true`. Los operadores ven toda la flota.

Con `[workspace] source = "store"`, `operations = true` y `oidc.groups_claim`, quien tiene un grupo que es dueño (`owner = "<grupo>"`) de una entrada de la versión actual del workspace que su token lee es operador de equipo. Ve lo que un lector con el mismo grant ve y puede guardar y aplicar cambios en las entradas de su equipo; cambiar cualquier otra entrada, un owner o una entrada sin owner se rechaza con un 403 `entries_refused` que nombra las entradas (un apply cuenta las que están fuera del grant). Pointer y fence siguen siendo del operador. Mapea el grupo del equipo a sus identidades en `[oidc.groups]`: ser dueño nunca amplía lo que un token lee. La página de versiones muestra el equipo responsable de cada entrada a todos los roles, y el editor del operador de equipo contiene solo las entradas que su token lee, marca un cambio que la API rechazaría (con su motivo) antes de poder guardarlo y nombra los equipos que lo autorizan en sus confirmaciones, en lugar de un comando de la CLI.

Las cuatro operaciones HTTP exigen OIDC y `operations = true`. Cada acción muestra el comando CLI y pide confirmación. Si se rechaza, muestra el mensaje del CLI, incluido el titular de un bloqueo ocupado. Cada intento genera una línea de auditoría en stderr, sin tokens ni bodies.

<details markdown="1" class="reference-details">
<summary>Grant, login y límites de recuperación</summary>

Las operaciones de la consola requieren OIDC y están desactivadas por defecto. Al habilitarlas, usan el mismo bloqueo y las mismas comprobaciones del CLI para cambiar el puntero, recuperar el fence y gestionar versiones. Cada intento genera una línea de auditoría. El grant también limita grupos y errores del manifiesto; el login conserva los parámetros del issuer, y la recuperación respeta el reloj y el límite del contador de Valkey.

</details>

## Auditoría

Con `[postgres]` en el bootstrap (y el schema al día con `meada postgres migrate`), cada escritura de la consola y cada comando del CLI que la refleja (`meada workspace import` y `apply`, `meada snapshot pointer set`, `retain` y `drop`, `meada snapshot fence recover`) deja un registro en Postgres: quién (sujeto, rol y, para un operador de equipo, los owners), qué (acción y argumentos), cuándo (el reloj de Postgres) y el resultado (`pending`, `accepted`, `refused` o `failed`). El registro se graba antes de que la escritura toque Valkey o el store: si no puede grabarse, la escritura se rechaza y nada cambia (la consola responde 503 `audit_unavailable`, el CLI sale con 1 nombrando Postgres). Los rechazos de rol y los rechazos bajo el lock también se registran; los bodies inválidos (400, 413, 422) no. La consola arranca con Postgres caído (las lecturas siguen) y se niega a arrancar con una versión de schema que no conoce. El operador global lee los registros, del más reciente al más antiguo, en la página **Audit log** o con `GET /api/v1/audit?before=<id>&limit=<1 a 100>`. La línea de auditoría en stderr no cambia, con o sin `[postgres]`, y Meada nunca borra un registro.

## Directorio (SCIM)

Con la feature `directory` (incluida en `cloud`), `[directory]` en el bootstrap, junto a `[postgres]` y `[oidc]`, hace que la consola sirva un endpoint SCIM 2.0 en `/scim/v2/`, al que el aprovisionamiento de Entra ID u Okta envía usuarios y grupos:

```toml
[directory]
token_env = "MEADA_SCIM_TOKEN"   # o token_file; la consola lo lee al arrancar
user_claim = "oid"
user_attribute = "externalId"    # o "userName"
group_attribute = "externalId"   # "displayName" por defecto
```

El endpoint acepta solo su propio bearer token, de al menos 32 caracteres: la consola no arranca con un token corto, ausente o ilegible, y rotarlo exige reiniciarla. Un token de la consola no sirve ahí, el token SCIM no sirve en `/api/v1/`, y cualquier petición con `Origin` recibe 403. Usuarios y grupos quedan en el schema de Postgres (ejecuta antes `meada postgres migrate`) tal como los envió el proveedor, nunca con `password`; un cuerpo de más de 1 MiB o un recurso de más de 64 KiB recibe 413. Borrar un usuario lo saca de todos los grupos y deja un marcador de revocación hasta que se lo aprovisione de nuevo con `active` verdadero. Cada escritura deja una línea en stderr, `scim: <método> <recurso> <id>: <status>`, sin valores. Todavía nada lee el directorio.

## Equipos

Varios equipos pueden compartir un workspace: cada uno modifica sus propias entradas y el operador global responde por el resto. Las reglas numeradas son todo el contrato.

1. **Confianza compartida.** El ownership por equipo es delegación, no aislamiento. Una entrada que un equipo añade la busca el job con las credenciales de la flota (una instalación de GitHub App o las credenciales git del job) y la lee todo grant `*`. Acepta esto antes de delegar.
2. **La clave `owner`.** Una entrada de versión del workspace puede llevar `owner = "<grupo OIDC>"`: de 1 a 256 caracteres, sin carácter de control ni espacio en blanco en los extremos, comparada exactamente, con mayúsculas y minúsculas. Un grupo que el proveedor envía con un carácter de control, o con más de 256 caracteres, no puede ser dueño de una entrada.

    ```toml
    [[repository]]
    repo = "github.exemplo.internal/acme/billing"
    path = "/var/lib/meada/mirrors/billing"
    owner = "team-billing"

    [[repository]]
    repo = "github.exemplo.internal/acme/api"
    path = "/var/lib/meada/mirrors/api"
    ```

    ```sh
    meada workspace import repositories.toml --message "Assign billing to its team" --workspace /etc/meada/job.workspace.toml
    ```

3. **Quién cambia qué.** El operador de equipo puede editar y eliminar las entradas que sus equipos poseen y añadir entradas cuyo owner sea uno de sus equipos. El operador global conserva los cambios de owner (asignar, borrar o sustituir), las entradas sin owner, las entradas de otros owners y todas las operaciones de generación (`pointer` y `fence`).
4. **Delegación.** Un grupo se vuelve equipo cuando el operador global le asigna su primera entrada. Un equipo que elimina su última entrada deja de serlo hasta que el operador global le asigne una de nuevo.
5. **Configuración.** Ser dueño nunca amplía lo que lee un token. Mapea el grupo de cada equipo en `[oidc.groups]` a las identidades que posee, y a una identidad antes de que el equipo pueda añadirla; sin eso, el equipo no ve sus entradas, y una adición fuera del grant se rechaza con `outside_grant`.

    ```toml
    [oidc.groups]
    "meada-operators" = ["*"]
    # Un equipo: mapea su grupo a las identidades que posee.
    "team-billing" = ["github.exemplo.internal/acme/billing"]
    ```

6. **Manifests estáticos.** `owner` en las entradas `[[repository]]` del propio manifest de bootstrap es un error: un manifest estático no tiene editor para delegar. Usa `[workspace] source = "store"` e importa las entradas.
7. **CLI.** `meada workspace import` y `apply` actúan como operador global: guardan las credenciales del store y de Valkey, que ya permiten toda escritura.
8. **Apply solo de owners.** La indexación nunca lee `owner`, así que aplicar una versión que solo cambia owners no genera una nueva generación. La vista de la flota sigue nombrando la versión que produjo primero la generación servida; no es un refresh atascado.
9. **Posición.** Los propios saves del operador de equipo nunca mueven una entrada (la versión guardada deja en su sitio las entradas fuera del grant). Puede aplicar una versión almacenada cuya única diferencia sea el orden de las entradas.
10. **Un repositorio con fallo.** Con la regla de aborto de la publicación completa, el repositorio con fallo de un equipo (credencial revocada, remoto borrado, fallo de parse) impide nuevas generaciones para todos los equipos hasta reparar la causa. Los nodos siguen sirviendo la última generación completa. Ver [Refresh](refresh.md).

## Operaciones por CLI

```sh
meada snapshot pointer set <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot pointer retain <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot pointer drop <generation> --workspace /etc/meada/console.workspace.toml
meada snapshot fence recover --workspace /etc/meada/console.workspace.toml
meada snapshot fence recover --at-least <fence> --workspace /etc/meada/console.workspace.toml
```

| Operación | Efecto |
| --- | --- |
| `set` | Selecciona una generación retenida. El próximo publish puede sustituirla; suspende el job para mantener el rollback. |
| `retain` | Protege una generación de la poda. El conjunto acepta hasta ocho generaciones. |
| `drop` | Elimina esa protección. La generación actual no puede eliminarse. |
| `fence recover` | Eleva el contador al mayor fence registrado, o al piso indicado, bajo bloqueo. El próximo publish usa un fence mayor. |

Si el fence de un nodo aislado no aparece en un heartbeat válido, pásalo con `--at-least` o elimina el `local_dir/current` de ese nodo para recibir la generación actual. No edites el contador manualmente.
