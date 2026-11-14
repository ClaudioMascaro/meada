# Consola de gestión

La consola usa la feature `ui`, incluida en `cloud`, y funciona en un proceso propio. Tiene editor, diff, playground y grafo. El editor guarda versiones de configuración; aplicar una versión y publicar índices son pasos separados.

Protege el acceso antes de exponer la consola. Sin `[oidc]` ni `[accounts]` ([cuentas locales](#cuentas-locales)), el proxy controla la admisión: quien alcance la consola fuera de esa protección podrá leer el grafo. Para empezar, activa las cuentas locales; para producción, el proveedor de identidad. El proxy debe conservar `Host`, `Origin` y `Authorization`. La consola acepta solo su Host público y, si hay Origin, su propio origen.

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

## Cuentas locales

Sin un proveedor de identidad, la consola puede autenticar sus propias cuentas (feature `accounts`, parte de `cloud`; ADR 0024). `[accounts]` exige `[postgres]` y solo está junto a `[oidc]` como break glass (abajo):

```toml
[accounts]
public_key_file = "/run/secrets/meada-signing.pub.pem"   # o public_key_env
private_key_file = "/run/secrets/meada-signing.pem"      # o private_key_env; solo la consola la lee
# admin_password_env = "MEADA_ADMIN_PASSWORD"            # el predeterminado; o admin_password_file
```

El par de claves es Ed25519 en PEM (`openssl genpkey -algorithm ed25519 -out meada-signing.pem` y `openssl pkey -in meada-signing.pem -pubout -out meada-signing.pub.pem`). La consola se niega a arrancar con una clave ausente, ilegible, que no sea Ed25519 o que no corresponda a la otra, nombrando la clave y nunca su valor. Ejecuta `meada postgres migrate` antes: en el primer arranque con la tabla vacía, la consola crea la cuenta predeterminada `admin` con el valor de `MEADA_ADMIN_PASSWORD`, sin cambio obligatorio, si pasa la política de contraseñas (de 12 a 128 caracteres, ni el nombre de usuario, ni una contraseña común), o, sin él, con una contraseña generada que debe cambiarse antes de cualquier otra cosa. La consola imprime la contraseña generada una sola vez, en una línea del stderr justo después de `listening on` (o, cuando el arranque falla después de crear la cuenta, antes de que la consola termine): `accounts: default account admin created; first password: <contraseña>`. Ningún arranque posterior la vuelve a imprimir, y nunca llega a la auditoría; quien la pierda recurre a `meada ui reset-admin`. Con `[accounts]`, `operations = true` no necesita `[oidc]` ni clave de operador: el rol es el de la cuenta.

`POST /api/v1/login` recibe `{"username", "password"}` y responde `{"token", "expires_at", "change_required"}`: un token de sesión que la consola firma y acepta durante ocho horas como bearer en toda ruta salvo los archivos estáticos, `config` y `login`. Un usuario desconocido, una contraseña errónea y una cuenta bloqueada reciben el mismo 401 `invalid_credentials`. Con el cambio pendiente, solo `POST /api/v1/account/password` (`{"current", "new"}`) acepta el token; devuelve uno nuevo. `config` incluye `"sign_in": "accounts"` y `session` incluye `"account": true`. Cada inicio de sesión, rechazo y cambio de contraseña queda en la auditoría (`account.login`, `account.password`) antes del token, sin contraseña ni token; sin el registro, la respuesta es 503 `audit_unavailable`. Limita la tasa de `/api/v1/login` en el proxy.

La política de contraseñas rechaza las contraseñas de una lista de contraseñas comunes incluida en el binario (de SecLists, licencia MIT, solo las entradas de 12 caracteres o más), sin distinguir mayúsculas, con la regla `common_password`. Diez fallos seguidos, al iniciar sesión o como contraseña actual de un cambio, bloquean la cuenta durante quince minutos; se cuentan en Postgres reteniendo la fila de la cuenta, así que los intentos en paralelo nunca pasan de diez, y un acierto reinicia la cuenta. Mientras está bloqueada, la cuenta recibe la respuesta de una contraseña errónea, en un tiempo similar, sin que se compruebe la contraseña y sin extender el bloqueo; la auditoría registra `locked_now` en el fallo que bloquea y `locked` después. Una contraseña guardada con parámetros más débiles que los de la consola se vuelve a cifrar en el siguiente inicio de sesión que la acierta.

Para recuperar la cuenta predeterminada (contraseña perdida o bloqueo), ejecuta `meada ui reset-admin --workspace <manifiesto de bootstrap>`: crea o reactiva `admin` como operador, quita el bloqueo y pone como contraseña el valor de `MEADA_ADMIN_PASSWORD` (o de `admin_password_file`) o, sin él, una contraseña aleatoria mostrada una vez en la salida estándar; el cambio queda obligatorio en el siguiente inicio de sesión. El comando registra `account.reset_admin` con `via` `cli` en la auditoría antes de cambiar la cuenta y sale con 1, sin cambiar nada, si no puede escribir el registro.

El operador global administra las cuentas por la API, con `operations` activado o no, para que una consola de solo lectura también tenga lectores. `GET /api/v1/accounts` lista todas (nombre de usuario, rol, estado `active`, `suspended` o `locked`, el fin de un bloqueo, si el cambio es obligatorio, el último cambio de contraseña, cuándo y quién la creó), nunca con el hash. `POST /api/v1/accounts` con `{"username", "role", "password", "change_required"}` crea una cuenta `operator` o `viewer` con una contraseña temporal que pasa la política; `change_required` es verdadero si se omite. Un nombre de usuario mal formado recibe 422 `invalid_username` y uno ya usado 409 `username_taken`. `POST /api/v1/accounts/<usuario>` hace una acción por vez: `{"action": "role", "role"}`, `{"action": "suspend"}`, `{"action": "reactivate"}` o `{"action": "reset_password", "password", "change_required"}`, que también quita el bloqueo y vuelve obligatorio el cambio si se omite `change_required`; una cuenta desconocida recibe 404. Suspender o degradar al último operador activo recibe 409 `last_operator`, incluso con dos solicitudes a la vez. Ninguna ruta borra ni renombra una cuenta. Quien no es el operador global recibe 403. Cada escritura de la consola lee la fila de la cuenta antes de actuar: una cuenta suspendida o degradada pierde sus escrituras al instante, y sus lecturas siguen hasta que el token expira; una cuenta suspendida recibe al iniciar sesión el mismo 401 que una contraseña errónea. Cada cambio queda en la auditoría (`account.create`, `account.role`, `account.suspend`, `account.reactivate`, `account.reset_password`) con la cuenta y los campos cambiados, nunca una contraseña, antes de hacerse; sin el registro, la respuesta es 503 `audit_unavailable` y nada cambia.

Los grupos de una cuenta los asigna el operador global (`"groups"` al crearla, y `{"action": "groups", "groups"}`, que reemplaza la lista; cada grupo es un nombre que puede tener el dueño de una entrada, si no 422 `invalid_group`), aparecen en la lista y quedan en la auditoría como `account.groups`. `[accounts.groups]` asocia cada grupo a las identidades que concede, como `[oidc.groups]` (`*` para todas):

```toml
[accounts.groups]
"team-billing" = ["github.com/acme/billing", "github.com/acme/ledger"]
"platform" = ["*"]
```

Sin la tabla, toda cuenta local lee todos los repositorios. Con ella, la concesión de una sesión local es lo que conceden los grupos de su token, y limita cada vista (el manifiesto, el workspace y sus versiones, el store, los repositorios, las credenciales, el playground, los exports y el grafo) como la de un proveedor de identidad; el operador global lee todo. Un lector cuyo grupo es dueño de una entrada de la versión actual del workspace que su concesión lee es operador de equipo (ADR 0015): `session` lista sus equipos, y guarda y aplica las entradas de sus equipos. El token de sesión lleva los grupos: un cambio vale para las escrituras al instante (se lee antes la fila de la cuenta) y para las lecturas en el próximo inicio de sesión. Con `[accounts]`, `GET /api/v1/admission` muestra el modo (`accounts` o `break_glass`), la huella de la clave pública (`blake3:` y el hash BLAKE3 de la clave Ed25519 cruda, como lo calcula `openssl pkey -pubin -in meada-signing.pub.pem -outform DER | tail -c 32 | b3sum`) y la tabla de grupos limitada por la concesión de quien pregunta.

En el navegador, `"sign_in": "accounts"` hace que la consola muestre su propia página de inicio de sesión en lugar de la redirección del proveedor de identidad. Un inicio de sesión que debe cambiar la contraseña va directo a una página que pide la contraseña actual y la nueva dos veces, muestra la regla que nombra el rechazo y no ofrece nada más hasta que el cambio tenga éxito, y entonces abre el workspace. El token de sesión vive en el `sessionStorage` de la pestaña (nunca en `localStorage`, una URL o un log), se envía como bearer y se olvida al salir, ante cualquier 401 que no sea una contraseña rechazada (la consola vuelve al inicio de sesión) y al cerrar la pestaña. La página de perfil ofrece el cambio voluntario de contraseña y la salida. El texto está en portugués e inglés.

La página de cuentas (`/accounts`) se lista solo para el operador global cuando `config` dice `"sign_in": "accounts"`; cualquier otro rol, y una consola bajo break glass, recibe la página de no encontrado en esa ruta. Lista cada cuenta con su rol, grupos, estado (una cuenta suspendida se marca en texto, una bloqueada dice hasta cuándo), si debe cambiar la contraseña y cuándo la cambió por última vez. El formulario de creación (nombre de usuario, rol, grupos, contraseña temporal y "debe cambiar la contraseña en el primer inicio de sesión", marcado por defecto) comprueba el patrón del nombre y la longitud de la contraseña antes de preguntar al servidor, y muestra la regla del servidor ante un rechazo. Rol, grupos, suspender, reactivar y restablecer la contraseña (con la misma casilla) pasan cada uno por el diálogo de confirmación compartido, y los rechazos del servidor, como `last_operator`, se muestran sin cambios. La página de auditoría nombra cada acción `account.*` en ambos idiomas.

### Tokens de agente

Una persona con sesión iniciada genera tokens de agente para sus agentes, para que cada agente consulte los nodos bajo la concesión de su dueño en lugar de un bearer compartido. `POST /api/v1/account/tokens` con `{"surface": "nodes", "hours": 24}` (de 1 a 168 horas, 8 cuando falta `hours`) responde `{"token", "expires_at", "surface"}` una sola vez: Meada no guarda ningún token de agente ni lo vuelve a mostrar. El token lleva la audiencia de los nodos (`meada:nodes`), el sujeto, el rol y los grupos tal como está ahora la fila de la cuenta, y un id aleatorio. Una sesión que debe cambiar su contraseña recibe el 403 `password_change_required`, y una cuenta suspendida el rechazo de rol (403 `forbidden`). Cada emisión queda registrada como `account.token` con la superficie, las horas y el id del token, nunca el token. La consola rechaza los tokens de agente (401): un token que vive hasta siete días y que nada revoca nunca hace escrituras en la consola. La página de perfil ofrece la superficie y la validez, y luego muestra el token con un control para copiarlo y su expiración; salir de la página lo borra. Cuando el manifiesto de bootstrap de la consola tiene `[proposals]`, `{"surface": "proposals"}` emite en cambio un token de agente para el [servidor de propuestas](named-workspaces.md#ejecutar-el-servidor-de-propuestas) (audiencia `meada:proposals`), que solo ese servidor admite, y `config` lleva `"proposals": true` para que el perfil lo ofrezca; sin la sección es un 422 `invalid_surface`.

Un nodo con `[accounts]` y sin `[oidc]` admite estos tokens en `meada mcp --listen`. Lee la clave pública al arrancar (nunca la privada) y comprueba el emisor, la `[ui] public_url` de la consola, así que su manifiesto lleva la sección `[ui]` de la consola junto a `[accounts]` y `[postgres]`, como el del job de actualización; una clave ausente o ilegible, o la falta de `[ui]`, rechaza el arranque. Un token ausente, expirado, aún no válido, firmado por otra clave o que no sea EdDSA, uno con `crit`, y un token de otra superficie (un token de sesión incluido) reciben 401 con `WWW-Authenticate: Bearer` antes del enrutamiento. La concesión es lo que `[accounts.groups]` asigna a los grupos del token (todos los repositorios sin la tabla), y limita el enrutamiento (403 fuera de la concesión), los listados 404 y toda respuesta, pins y rutas de workspaces con nombre incluidos, como lo hace el token de un proveedor de identidad. Rotar el par de claves termina de una vez todos los tokens en circulación; sin eso, un token dura hasta expirar, incluso después de suspender su cuenta. Configure un cliente MCP con el token como bearer:

```json
{"mcpServers": {"meada": {"type": "http", "url": "https://nodes.example/github.com/acme/web",
  "headers": {"Authorization": "Bearer <token de agente>"}}}}
```

### Login de GitHub verificado

Con `[accounts.github]`, el proveedor de concesión de GitHub de las cuentas locales, una persona prueba su login de GitHub una vez y sus roles en GitHub pasan a concederle repositorios, como lo hace `login_claim` con un proveedor de identidad. La sección tiene las claves de `[oidc.github]` sin `login_claim` (`host`, `roles`, `max_age_seconds`) y `api`, el endpoint REST de GitHub de ese host (`https://api.github.com` por defecto en `github.com`, obligatorio en cualquier otro host, `https` o `http` en loopback):

```toml
[accounts.github]
host = "github.com"
roles = ["write", "maintain", "admin"]
max_age_seconds = 5400
```

En la página de perfil, la persona pega un token de acceso personal de GitHub (basta uno de granularidad fina sin ningún permiso). `POST /api/v1/account/github` con `{"token": "..."}` hace que la consola llame a `GET /user` en `api` una sola vez, guarde el login en minúsculas y descarte el token: nunca se guarda, se registra en logs ni en la auditoría, ni se envía a otro lugar. La respuesta trae el login y un token de sesión nuevo que lo lleva. Un login que otra cuenta ya verificó es un 409 `login_taken`; el rechazo de GitHub es un 422 `github_refused`, y GitHub fuera de alcance un 503 `github_unavailable`, ninguno repitiendo la respuesta de GitHub. `POST /api/v1/account/github/remove` quita el propio login, y el operador global quita el de cualquier cuenta con la acción `remove_github` de la página de cuentas. Los tokens de sesión y de agente llevan el login verificado, y la concesión de GitHub, leída del conjunto de permisos que graba la comprobación de roles del job (`meada github-app roles` lee `[accounts.github]`), se suma a la de los grupos en la consola, en los nodos, en las adiciones de un operador de equipo (`added_by`) y en la columna `login` de la auditoría. Un token emitido antes de una remoción mantiene el login hasta expirar. Cada verificación y remoción queda registrada como `account.github` (con el login) y `account.remove_github`.

### Break glass

Al configurar `[oidc]`, el inicio de sesión local se apaga para todas las cuentas: `[accounts]` junto a `[oidc]` es un error, salvo que lleve `break_glass = true`. Entonces solo la cuenta predeterminada `admin` entra en la consola, como operador global, junto a los tokens del proveedor de identidad, para cuando este está caído:

```toml
[accounts]
break_glass = true
public_key_file = "/run/secrets/meada-signing.pub.pem"
private_key_file = "/run/secrets/meada-signing.pem"
```

`break_glass` sin `[oidc]` es un error, y `[accounts.groups]` y `[accounts.github]` no pueden estar bajo él: la cuenta de break glass no necesita concesión. Cualquier otro nombre de usuario recibe el mismo 401 que una contraseña equivocada, y la auditoría registra `not_break_glass`. La consola distingue los bearers por emisor antes de probar cualquier clave: un token cuyo `iss` es la `public_url` de la consola solo se verifica con la clave Ed25519 (y solo vale el de `admin`), cualquier otro solo con las claves del proveedor de identidad; por eso la consola rechaza el arranque si el `issuer` de `[oidc]` es su propia `public_url`. La cuenta de break glass cambia su propia contraseña; la administración de cuentas, los tokens de agente y la verificación de login no existen (404). Los nodos y el servidor de propuestas ignoran `[accounts]` junto a `[oidc]` y nunca leen la clave privada. `config` trae `"sign_in": "oidc"` con `"break_glass": true`, y el navegador, en lugar de redirigir directo al proveedor de identidad, muestra su botón y, discreto, un enlace a `/break-glass`, la página de inicio de sesión de la cuenta predeterminada; el enlace sigue ahí cuando el proveedor de identidad no responde.

### Seguridad de las cuentas locales

- **Contraseñas:** guardadas con argon2id (19 MiB, dos iteraciones, un lane; los parámetros quedan en el hash y un hash más débil se rehace en el siguiente login). Solo un número pequeño de hashes corre a la vez: el excedente recibe 503 `busy` sin calcular nada.
- **Bloqueo:** diez fallos seguidos bloquean la cuenta por quince minutos; un restablecimiento por el operador global o `meada ui reset-admin` quita el bloqueo.
- **Duración de los tokens:** el token de sesión vale ocho horas; el de agente, de 1 a 168 horas (8 por defecto).
- **Sin lista de revocación:** Meada no guarda tokens de agente, así que la validez es la revocación. Suspender o degradar una cuenta bloquea sus escrituras en la consola al instante (la fila se lee en cada escritura) y la emisión de tokens nuevos, pero cambiar la contraseña no invalida nada, y un token ya emitido sigue admitido en los nodos y en el servidor de propuestas hasta expirar.
- **Rotación de claves como parada de emergencia:** cambia el par Ed25519 y reinicia la consola, los nodos y el servidor de propuestas: todo token en circulación, de sesión o de agente, deja de valer de una vez. No hay rotación con claves superpuestas. Solo la consola lee la clave privada.
- **Token de acceso personal de GitHub:** se lee una vez, para obtener el login en `GET /user`, y se descarta; nunca se guarda, ni se registra en log ni en la auditoría.
- **Límite de tasa en el proxy:** la consola no limita por dirección. Limita `POST /api/v1/login` (y, si quieres, `account/password`) en el proxy, por ejemplo con `limit_req_zone $binary_remote_addr zone=login:10m rate=5r/m;` y `limit_req zone=login burst=5;` en el `location` del login en nginx. Los fallos de nombres desconocidos entran en la auditoría y la hacen crecer sin límite, y el bloqueo por cuenta no los frena.
- **Lo que no existe:** correo, registro propio, "olvidé la contraseña", autenticación multifactor, lista de revocación, listado de tokens emitidos, y borrar o renombrar cuentas. Quien pierde la contraseña de `admin` usa `meada ui reset-admin`.

### Migrar de las cuentas locales al proveedor de identidad

Al configurar `[oidc]`, el login local se desactiva para todas las cuentas (mira [Break glass](#break-glass)). Pasos:

1. Configura `[oidc]` en la consola y en los nodos ([Autenticación y grants](admission.md)) y añade `break_glass = true` a `[accounts]`, para mantener la cuenta predeterminada como salida de emergencia. Quita `[accounts.groups]` y `[accounts.github]`: bajo break glass son un error.
2. Copia la tabla de `[accounts.groups]` a `[oidc.groups]`, cambiando el nombre de cada grupo local por el valor que la claim `groups_claim` del proveedor de identidad lleva (en Entra, el id del grupo). Los grupos locales y los del proveedor son nombres independientes: el mapeo es manual, y la concesión de un token pasa a ser la de los grupos que emite el proveedor. Los `owner` de las entradas del workspace son nombres de grupo y siguen valiendo si el proveedor usa los mismos nombres; si no, actualízalos.
3. Pasa `[accounts.github]` a `[oidc.github]`: `host`, `roles` y `max_age_seconds` se quedan, `api` sale, y el `login_claim` reemplaza el login verificado, que ahora mapea el administrador del proveedor (mira las [recetas](identity-providers.md)).
4. Quien era operador local pasa a ser operador por `ui.operator_groups` o `ui.operator_claim`.

Lo que deja de funcionar: el inicio de sesión de cualquier cuenta que no sea `admin`; los tokens de sesión y de agente de las cuentas locales (los nodos y el servidor de propuestas pasan a admitir solo los tokens del proveedor, y los agentes pasan a usar tokens del proveedor de identidad); la página de cuentas, la emisión de tokens de agente y la verificación del login (404); y los sujetos `local:<usuario>` registrados como autores, operadores de workspaces con nombre y registros de auditoría, que nunca coinciden con los del proveedor, así que rehaz la lista de operadores de cada workspace con nombre. Las cuentas siguen en la tabla, inertes. Ninguna cuenta ni grupo se migra solo.

## Infraestructura en el grafo

En **Grafo de código**, carga la identidad de un repositorio y, opcionalmente, una generación.
**Contratos** muestra aprovisionadores con su dirección IaC y raíz de evaluación (módulo u
overlay), junto a proveedores, consumidores y usos de configuración. **Enlaces de
aprovisionamiento** conserva cada uso enlazado con cada aprovisionador, con raíz, nivel y motivo.

**Enlaces de imágenes** muestra constructor → ejecutor: el repositorio que publica la imagen y
la carga de trabajo que la ejecuta. **Enlaces de alertas** muestra emisor → matcher: la etiqueta
o tag y la condición que la comprueba. **Enlaces de CI** muestra pipeline o action →
llamador: el pipeline reutilizable o la action y el job de CI o paso de CI que lo llama, con
el ref Git tal como está escrito y el commit indexado en el motivo; una llamada cuya ruta no
nombra proveedor aparece en la tabla de contratos con la marca
` (absent at the indexed commit)`. Estas vistas incluyen enlaces dentro del mismo
repositorio y entre repositorios; la vista general conserva su alcance entre repositorios.
Las tablas de contratos de cada tipo también muestran lados sin contraparte, sin inventar
enlaces para ellos. La leyenda explica los roles y las tablas de enlaces conservan nivel y motivo.

Usa Tab para llegar al selector de vista y las tablas; las flechas eligen la vista y desplazan
la tabla enfocada. Cada tabla tiene nombre accesible y encabezados. Las vistas de infraestructura
aparecen cuando el export contiene los lados o enlaces correspondientes. Recargar conserva la
vista seleccionada, o vuelve a comunidades si dejó de estar disponible. El playground ofrece los filtros `value`, `iac`, `alert` y `ci`, junto a `image`
y los tipos existentes, y muestra la respuesta de la API sin reescribirla. La interfaz está en
portugués e inglés.

## Flota y diagnóstico

Cada `snapshot pull --every` envía un heartbeat, el registro periódico de estado del nodo. Informa generación, fence, generaciones locales, disponibilidad, versión y hora.

Usa un `snapshot.node_name` único para cada nodo y la consola, con hasta 63 letras minúsculas, dígitos, puntos o guiones. Sin él, el proceso usa un hostname válido. Los nombres de pods pueden cumplir esa función.

Un nodo sin heartbeat no aparece en la flota. Heartbeats malformados aparecen separados y no entran en el diagnóstico. Las páginas muestran flota, store, retención, commits, schemas, caché de Apps y admisión; la consola identifica las secciones ausentes.

## Roles y operaciones

Con OIDC, los lectores ven solo las identidades admitidas por su token. Para conceder el rol de operador, elige **una** opción: grupo en `ui.operator_groups` o claim en `ui.operator_claim` con valor booleano `true`. Los operadores ven toda la flota.

Con `[workspace] source = "store"`, `operations = true` y `oidc.groups_claim`, quien tiene un grupo que es dueño (`owner = "<grupo>"`) de una entrada de la versión actual del workspace que su token lee es operador de equipo. Ve lo que un lector con el mismo grant ve y puede guardar y aplicar cambios en las entradas de su equipo; cambiar cualquier otra entrada, un owner o una entrada sin owner se rechaza con un 403 `entries_refused` que nombra las entradas (un apply cuenta las que están fuera del grant). Pointer y fence siguen siendo del operador. Mapea el grupo del equipo a sus identidades en `[oidc.groups]`: ser dueño nunca amplía lo que un token lee. La página de versiones muestra el equipo responsable de cada entrada a todos los roles, y el editor del operador de equipo contiene solo las entradas que su token lee, marca un cambio que la API rechazaría (con su motivo) antes de poder guardarlo y nombra los equipos que lo autorizan en sus confirmaciones, en lugar de un comando de la CLI.

Las cuatro operaciones HTTP exigen `operations = true` y un rol de operador, que viene de OIDC o de una [cuenta local](#cuentas-locales) (nunca solo del proxy). Cada acción muestra el comando CLI y pide confirmación. Si se rechaza, muestra el mensaje del CLI, incluido el titular de un bloqueo ocupado. Cada intento genera una línea de auditoría en stderr, sin tokens ni bodies.

<details markdown="1" class="reference-details">
<summary>Grant, login y límites de recuperación</summary>

Las operaciones de la consola requieren `[oidc]` o `[accounts]` y están desactivadas por defecto. Al habilitarlas, usan el mismo bloqueo y las mismas comprobaciones del CLI para cambiar el puntero, recuperar el fence y gestionar versiones. Cada intento genera una línea de auditoría. El grant también limita grupos y errores del manifiesto; el login conserva los parámetros del issuer, y la recuperación respeta el reloj y el límite del contador de Valkey.

</details>

## Auditoría

Con `[postgres]` en el bootstrap (y el schema al día con `meada postgres migrate`), cada escritura de la consola y cada comando del CLI que la refleja (`meada workspace import` y `apply`, `meada snapshot pointer set`, `retain` y `drop`, `meada snapshot fence recover`) deja un registro en Postgres: quién (sujeto, rol y, para un operador de equipo, los owners), qué (acción y argumentos), cuándo (el reloj de Postgres) y el resultado (`pending`, `accepted`, `refused` o `failed`). El registro se graba antes de que la escritura toque Valkey o el store: si no puede grabarse, la escritura se rechaza y nada cambia (la consola responde 503 `audit_unavailable`, el CLI sale con 1 nombrando Postgres). Los rechazos de rol y los rechazos bajo el lock también se registran; los bodies inválidos (400, 413, 422) no. La consola arranca con Postgres caído (las lecturas siguen) y se niega a arrancar con una versión de schema que no conoce. El operador global lee los registros, del más reciente al más antiguo, en la página **Audit log** o con `GET /api/v1/audit?before=<id>&limit=<1 a 100>`. La línea de auditoría en stderr no cambia, con o sin `[postgres]`, y Meada nunca borra un registro.

## Catálogo de repositorios

Con `[catalog]` en el bootstrap (feature `workspaces`, junto a `[oidc.github]` o `[accounts.github]`; vea [el catálogo](github-app.md#el-catalogo-de-repositorios)),
la consola ofrece el **Catálogo de repositorios** a todo sujeto admitido, y `GET /api/v1/config` trae `"catalog": true`.
Es el selector que viene antes de cualquier manifiesto: lista solo los repositorios que la persona puede seleccionar, es
decir, los que el catálogo contiene y en los que su login tiene un rol aceptado en el conjunto de permisos más reciente
(el administrador de la plataforma ve el catálogo entero). No crea workspace, lista de acceso ni índice.

- `GET /api/v1/catalog?limit=&after=&q=` responde `{"state", "reason", "set", "repositories": [{"identity"}], "next"}`,
  ordenado por identidad, con `limit` de 1 a 100 (50 por defecto), `q` un prefijo de hasta 128 caracteres y `after` el
  cursor opaco (hasta 256 caracteres) que devolvió `next`. Cualquier otro parámetro, uno repetido o un valor fuera de
  rango es un 400 con mensaje fijo.
- `POST /api/v1/catalog/check` recibe `{"identities": [...]}` (hasta 50, 16 KiB) y responde `{"state", "reason",
  "results": [{"identity", "state"}]}` en el orden pedido: `selectable`, `unavailable` o `pending`.
- `state` es `ready` o `pending`. **Pendiente** es una condición de la plataforma o de quien pregunta, igual para toda
  identidad: `no_set` (el job aún no escribió un conjunto), `stale_set` (el conjunto superó `max_age_seconds`, también
  para el administrador) o `missing_login` (el token no trae un login válido). Mientras está pendiente no se lista nada
  y nada nuevo puede seleccionarse; las entradas que un manifiesto ya tiene permanecen.
- `unavailable` es todo otro caso (sin rol aceptado, fuera del catálogo, no cubierto, nuevo desde el último conjunto) y
  **no trae motivo**: un motivo sería un oráculo sobre el catálogo. La lectura nunca lista un repositorio no disponible,
  y una identidad que no existe responde igual que una que la persona no puede seleccionar.

Las dos rutas exigen un sujeto admitido y no desactivado (el bloqueo por SCIM rige aquí como en toda ruta), y la consola
las responde del conjunto que ya tiene en memoria, sin llamar a GitHub ni a Postgres. `GET /api/v1/manifest` solo muestra
`catalog.allow` al operador global.

## Workspaces con nombre

Con `[workspaces]` en el bootstrap (feature `workspaces`, junto a `[ui] operations = true`, `[oidc.github]` (o `[accounts.github]`), `[catalog]` y `[workspace] source = "store"`; `mirrors` es la raíz absoluta donde se espejan los repositorios que solo seleccionan workspaces con nombre, `max_workspaces` vale 200 por defecto y `max_entries` 100), `GET /api/v1/config` trae `"workspaces": {"max_entries": n}` y la consola ofrece la página **Workspaces con nombre** a todo sujeto admitido.

- `POST /api/v1/workspaces` recibe `{"name", "description"?, "entries", "message"}` (80 KiB), siendo `entries` el texto del manifiesto con entradas `[[repository]]` que tienen `repo` y, como máximo, bloques `serve`, `resource` y `peer` (`path`, `owner`, `added_by` y `serve.documents` se rechazan, 422 `invalid_entries`). El creador es el sujeto autenticado: `actor`, `role` o cualquier otra clave en el cuerpo es un 400. Responde 201 `{"id", "version", "created": true}`, o 200 con `created: false` cuando el mismo creador, nombre y entradas ya lo crearon. Una selección con un repositorio fuera de la elegibilidad de la persona es un 403 `entries_refused` (`refused`: las identidades que la concesión de la persona lee, `hidden`: cuántas otras) y no guarda nada; un conjunto que no puede decidir es un 409 `pending` con `reason` (`no_set`, `stale_set`, `missing_login`). La creación queda en la auditoría como `workspaces.create`, incluidos los rechazos.
- `GET /api/v1/workspaces?limit=&after=` lista los workspaces que el sujeto ve, ordenados por id (`limit` de 1 a 100, 25 por defecto), y `GET /api/v1/workspaces/<id>` muestra uno con su `base`, su `state` (`applied`: guardado, ingestión pendiente; `live`: la generación actual lo registra) y la `ingestion` de cada entrada (`pending`, `ready` o `excluded`). Un workspace es visible para sus operadores, para el administrador de la plataforma y para quien tiene una concesión que lee todos sus repositorios; para los demás responde el mismo 404 que un id que no existe. Los operadores también reciben la lista de operadores.
- El playground acepta `"workspace": "<id>"` y `GET /api/v1/workspaces/<id>/repositories/<identity>/export` devuelve el export a través de ese workspace, bajo la misma regla y siempre acotado por la concesión de quien pregunta.
- El **ciclo de vida** (#343): `POST /api/v1/workspaces/<id>/versions` `{"entries", "message", "base"}` (80 KiB) guarda una versión sin mover nada (201, o 200 `saved: false` cuando la versión ya estaba guardada); `.../apply` `{"version", "base"}` (1 KiB) hace de una versión guardada la actual y responde `action` `apply` o `rollback` (una versión que ya fue la actual) con la nueva `base`; `.../rename` `{"name", "description"?, "base"}`; `.../delete` `{"base"}` (las versiones y el historial se conservan); `.../operators` `{"add": [{"subject", "login"}], "remove": [subject], "base"}`, solo del administrador de la plataforma. `base` es la que devuelve `GET /api/v1/workspaces/<id>`; una base vieja es un 409 `stale_base` que indica la base actual. Otros rechazos: 403 `not_operator` o `not_administrator`, 403 `entries_refused` (con `hidden`) para una entrada para la que la persona no es elegible, 409 `pending` cuando el conjunto de permisos no puede decidir, 409 `already_current`, 404 `unknown_version`, 422 `invalid_entries`, `invalid_workspace` o `invalid_operators` (`last_operator`, `too_many_operators`); quien no puede ver el workspace recibe el 404 de un id desconocido. Un operador envía las entradas que lee y la versión guardada conserva las que no lee; el administrador de la plataforma envía el manifiesto entero. Cada operación es un registro de auditoría, `workspaces.save`, `.apply`, `.rollback`, `.rename`, `.delete` u `.operators`, rechazos incluidos, y el bloqueo SCIM de un usuario borrado vale para todas.
- `GET /api/v1/workspaces/<id>/versions?after=` lista las versiones (50 por página, de la más nueva a la más antigua, la actual marcada; los operadores también ven el autor), `GET .../versions/<version>?against=<versión o current>` muestra una con sus entradas, su `text` de manifiesto y, con `against`, el diff (`added`, `removed`, `changed`), y `GET .../history?after=<seq>` lista el historial, solo de añadidos, para los operadores y el administrador. Todas ocultan lo que la concesión de quien pregunta no lee: un lector ve solo las versiones que lee enteras, un operador las entradas que lee y un recuento `hidden`.
- `meada workspaces list|show|versions|diff|create|save|apply|rename|delete|operators --workspace <manifiesto de bootstrap>` son los gemelos de línea de comandos, que actúan como administrador de la plataforma con `cli:<login>@<host>` como actor y la misma auditoría; toda escritura recibe la `--base` que leyó.
- La **configuración efectiva** (#344): `GET /api/v1/workspaces/<id>/effective[?version=]` muestra, por entrada que quien llama lee, la `source` de la configuración (`repository`: el `meada.toml` del mirror gana por completo; `context`: los bloques del workspace; `none`), los `blocks` resueltos, los `ignored`, la `coverage` (`indexed`, `pending_ingestion`, `excluded`) y `diagnostics` (`ignored_by_repository`, `unknown_until_ingested`, `excluded_no_sponsor`, `peer_missing`, `documents_not_indexed`), con la `generation` de donde vienen los hechos, el `state` del workspace y `recorded` (si esa generación registró el contexto en la versión mostrada; si no, los bloques llegan a las consultas tras el siguiente refresh); `POST /api/v1/workspaces/effective` `{"entries"}` (80 KiB) muestra la misma vista para un texto de manifiesto antes de guardarlo, no guarda ni audita nada, y muestra como pendiente un repositorio que el grant de quien llama no lee. Una creación, un guardado y una aplicación responden con los mismos `diagnostics` (`[{repo, code, detail?}]`) junto al resultado. La página de workspaces tiene un editor de texto para el manifiesto de un workspace que el sujeto opera: valida con el mismo WebAssembly del servidor, muestra la configuración efectiva y luego guarda y aplica en la base que leyó.
- La **autoría guiada** (#345): la misma página edita el manifiesto con controles de formulario (repositorios del catálogo, ajustes de serve, recursos, peers y nombres de variables de entorno) que son un solo documento con el texto: ambos pasan por el WebAssembly de `meada-core` (`namedEntriesJson`, `namedEntriesText`, `namedDiff`, `namedChanged`, `namedAuthorize`), así que el formulario no agrega política. Muestra en texto la elegibilidad, la ingesta y la configuración efectiva de cada entrada, el diff del borrador contra la versión guardada, y congela el contenido revisado (texto, mensaje, `base`) hasta que se confirme *Guardar y aplicar*. No necesita ruta nueva: lee el catálogo (`GET /api/v1/catalog`, `POST /api/v1/catalog/check`) y usa las rutas anteriores; un `403 entries_refused` lee el catálogo de nuevo, un `409 stale_base` ofrece recargar y un `403 not_operator` retira el editor. Quien solo ve no tiene controles de cambio.

## Directorio (SCIM)

Con la feature `directory` (incluida en `cloud`), `[directory]` en el bootstrap, junto a `[postgres]` y `[oidc]`, hace que la consola sirva un endpoint SCIM 2.0 en `/scim/v2/`, al que el aprovisionamiento de Entra ID u Okta envía usuarios y grupos:

```toml
[directory]
token_env = "MEADA_SCIM_TOKEN"   # o token_file; la consola lo lee al arrancar
user_claim = "oid"
user_attribute = "externalId"    # o "userName"
group_attribute = "externalId"   # "displayName" por defecto
```

El endpoint acepta solo su propio bearer token, de al menos 32 caracteres: la consola no arranca con un token corto, ausente o ilegible, y rotarlo exige reiniciarla. Un token de la consola no sirve ahí, el token SCIM no sirve en `/api/v1/`, y cualquier petición con `Origin` recibe 403. Usuarios y grupos quedan en el schema de Postgres (ejecuta antes `meada postgres migrate`) tal como los envió el proveedor, nunca con `password`; un cuerpo de más de 1 MiB o un recurso de más de 64 KiB recibe 413. Borrar un usuario lo saca de todos los grupos y deja un marcador de revocación hasta que se lo aprovisione de nuevo con `active` verdadero. Cada escritura deja una línea en stderr, `scim: <método> <recurso> <id>: <status>`, sin valores. La consola lee el directorio en cada solicitud: un usuario cuyo valor de `user_claim` encuentra un usuario inactivo (o un marcador de revocación) recibe 403 `deprovisioned` en toda ruta salvo `GET /api/v1/config`, sea cual sea su rol, y la consola muestra una página de cuenta desactivada; un usuario nunca provisionado es admitido. Con el directorio ilegible, las lecturas se admiten y las escrituras reciben 503 `audit_unavailable`. El editor del workspace ofrece los grupos del directorio como owners solo al operador global, `owner_missing` marca en texto las entradas cuyo owner no nombra ningún grupo (una vez provisionado algún grupo), y la página de admisión muestra los conteos de usuarios y grupos y la última escritura SCIM.

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

    Una entrada también puede llevar `added_by = "<login de GitHub>"`, el login de quien la añadió: de 1 a 39 caracteres entre letras ASCII minúsculas, dígitos, `-` y `_` (un login de Enterprise Managed Users como `ana_acme` sirve), comparado exactamente, así que `Ana` se rechaza. Va después del `owner` en `meada workspace show` y entra en el id de la versión. Con `[oidc.github]`, el guardado de un operador de equipo en la consola define el `added_by` de cada adición en el `host` del proveedor de grant de GitHub como el login de quien guarda, diga lo que diga el texto enviado (y lo borra en las adiciones fuera de él); el refresh solo busca la entrada mientras ese login tenga un rol aceptado, y las vistas del workspace marcan `excluded` la entrada que la última verificación de roles dejó fuera. Una adición en el `host` sin `added_by`, o de un token sin login, se rechaza con `unattributed`, y una edición que cambia, borra o define el `added_by` con `added_by_change`. El operador global y el CLI guardan el `added_by` tal como está escrito: definirlo, borrarlo o sustituirlo es como adopta la adición de un equipo. En las entradas `[[repository]]` del propio manifest de bootstrap es un error, como el `owner`. Con `[oidc.github]`, cada `added_by` que escribe el guardado de un operador de equipo también recibe `added_by_time = "YYYY-MM-DDTHH:MM:SSZ"`, el segundo UTC en que la consola lo escribió (su reloj, nunca el texto; se conserva mientras el `added_by` sigue), justo después del `added_by` y parte del id de la versión; con `[oidc.github]` la verificación de roles reporta `unattributed` la adición cuyo `added_by` no tiene hora o tiene una que no pasa del primer conjunto de permisos del proveedor (ver la guía enterprise).

3. **Quién cambia qué.** El operador de equipo puede editar y eliminar las entradas que sus equipos poseen y añadir entradas cuyo owner sea uno de sus equipos. El operador global conserva los cambios de owner (asignar, borrar o sustituir), las entradas sin owner, las entradas de otros owners y todas las operaciones de generación (`pointer` y `fence`).
4. **Delegación.** Un grupo se vuelve equipo cuando el operador global le asigna su primera entrada. Un equipo que elimina su última entrada deja de serlo hasta que el operador global le asigne una de nuevo.
5. **Configuración.** Ser dueño nunca amplía lo que lee un token. Mapea el grupo de cada equipo en `[oidc.groups]` a las identidades que posee, y a una identidad antes de que el equipo pueda añadirla; sin eso, el equipo no ve sus entradas, y una adición fuera del grant se rechaza con `outside_grant`. Con `[oidc.github]`, una adición en el `host` del proveedor de grant de GitHub no necesita ese mapeo, pero una identidad que el workspace ya tiene fuera del grant se sigue rechazando con `outside_grant` al guardar, lo que le revela al equipo que existe.

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
