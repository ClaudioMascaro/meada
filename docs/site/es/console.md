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

Con `[workspace] source = "store"`, `operations = true` y `oidc.groups_claim`, quien tiene un grupo que es dueño (`owner = "<grupo>"`) de una entrada de la versión actual del workspace que su token lee es operador de equipo. Ve lo que un lector con el mismo grant ve y puede guardar y aplicar cambios en las entradas de su equipo; cambiar cualquier otra entrada, un owner o una entrada sin owner se rechaza con un 403 `entries_refused` que nombra las entradas (un apply cuenta las que están fuera del grant). Pointer y fence siguen siendo del operador. Mapea el grupo del equipo a sus identidades en `[oidc.groups]`: ser dueño nunca amplía lo que un token lee.

Las cuatro operaciones HTTP exigen OIDC y `operations = true`. Cada acción muestra el comando CLI y pide confirmación. Si se rechaza, muestra el mensaje del CLI, incluido el titular de un bloqueo ocupado. Cada intento genera una línea de auditoría en stderr, sin tokens ni bodies.

<details markdown="1" class="reference-details">
<summary>Grant, login y límites de recuperación</summary>

Las operaciones de la consola requieren OIDC y están desactivadas por defecto. Al habilitarlas, usan el mismo bloqueo y las mismas comprobaciones del CLI para cambiar el puntero, recuperar el fence y gestionar versiones. Cada intento genera una línea de auditoría. El grant también limita grupos y errores del manifiesto; el login conserva los parámetros del issuer, y la recuperación respeta el reloj y el límite del contador de Valkey.

</details>

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
