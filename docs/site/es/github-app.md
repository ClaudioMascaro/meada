# Credenciales de GitHub App

**GitHub App** (feature `github-app`, que incluye Valkey; `cargo install meada --features
github-app`). Meada sigue sin buscar nada: el git del job de indexación hace el fetch de los
espejos, y Meada solo responde a la petición de credencial de git con tokens de instalación de
uno o más GitHub Apps, que él emite y renueva. Las instalaciones se descubren por la API de
GitHub, no se declaran; la carga se reparte entre los Apps y las instalaciones, y una instalación
limitada queda de lado hasta liberarse. Solo el job usa el módulo: `meada mcp --listen` lee los
bloques con el manifiesto y nunca llama a GitHub.


<!-- meada-graph:app-credentials -->

```toml
[valkey]                                # obligatorio con [[github_app]]
url = "rediss://meada@cache.ejemplo.internal:6379"
password_file = "/run/secrets/valkey"

[[github_app]]
host = "github.com"                     # el host git que atiende, como git lo nombra
api = "https://api.github.com"          # la URL base de la API REST; en GHES, https://<host>/api/v3
app_id = 123456
private_key_file = "/run/secrets/github-app.pem"   # o private_key_env, nunca los dos
```

## Apps e instalaciones

Puede haber varios bloques `[[github_app]]`, uno por App (dos bloques del mismo App en el mismo
host son un error). `api` tiene que ser `https` fuera de loopback, y un bloque sin `[valkey]`,
sin clave o con las dos, o con una clave desconocida rechaza la ejecución nombrando la clave o la
sección. La clave privada solo viene del archivo o de la variable y nunca aparece en stderr ni en
un error, igual que el JWT del App y los tokens.

## Configurar el helper

En el job, antes del fetch, git usa Meada como credential helper del host, con la ruta del
repositorio en la petición (use la ruta absoluta del manifiesto, ya que git ejecuta el helper en
el directorio del espejo):

```sh
git config --global credential.https://github.com.helper \
  "!meada github-app credential --workspace /etc/meada/meada.workspace.toml"
git config --global credential.useHttpPath true
```

<details markdown="1" class="reference-details">
<summary>Selección de instalación, caché y renovación del token</summary>

Para `https://github.com/<owner>/<repo>`, el `get` responde `username=x-access-token`, un token
de una instalación que cubre a `<owner>` y su `password_expiry_utc`. Las instalaciones de cada
App se listan con su JWT (RS256, emitido 60 s en el pasado, que expira en 9 minutos) y se guardan
en Valkey, de nuevo como mucho cada 10 minutos; un bloque nuevo entra en el siguiente `get`, sin
reiniciar nada. Entre las instalaciones que cubren al owner, en un orden estable (id del App,
luego id de la instalación), Meada salta las marcadas como limitadas y elige una por turnos, con
un contador en Valkey. Un token solo se emite cuando Valkey no guarda uno de esa instalación con
al menos 10 minutos de vida; un lock por instalación hace que `get`s simultáneos emitan una sola
vez, y el token queda en caché hasta 10 minutos antes de expirar. Quien lee ese Valkey lee el
código fuente durante ese tiempo. Un repositorio que ninguna instalación cubre no recibe
respuesta, con salida 0 (git prueba el siguiente helper) y una línea en stderr con el host y el
owner. `store` no hace nada; el `erase`, que git manda después de un 401, descarta el token
rechazado, y el siguiente `get` emite otro.

</details>

## Límites de peticiones

Ni git ni GitHub avisan al helper de un límite secundario (403 o 429). Por eso, después de un
fetch que falló así, el job ejecuta:

```sh
meada github-app throttled --workspace /etc/meada/meada.workspace.toml \
  https://github.com/<owner>/<repo> --retry-after <segundos>
```

Eso marca como limitada la instalación que respondió por última vez para ese repositorio (Meada
lo recuerda durante una hora), hasta el `--retry-after` o, sin él, durante 1 minuto, duplicando
en cada repetición mientras la marca vale. Las llamadas REST del propio módulo que reciben 403 o
429 marcan su instalación (un token) o su App (el listado) del mismo modo, por el `retry-after`,
si no por el `x-ratelimit-reset` cuando `x-ratelimit-remaining` es 0, si no durante 1 minuto; un
`get` cuyo token fue limitado pasa a la siguiente instalación. Con git 2.54 o posterior,
`http.maxRetries` hace que el propio git respete el `Retry-After` de un 429 en sus peticiones
GET.

En GitHub.com y GitHub Enterprise Cloud, `api` queda fuera de la frontera del operador; se acepta
porque los fetches del job ya van a ese host (ADR 0013).

## Verificación de roles

Con `[oidc.github]` (features `github-app`, `postgres` y `oidc`), el job le pregunta a GitHub, en
cada refresh, quién tiene qué rol en cada repositorio del workspace en el `host` del proveedor,
con las mismas instalaciones, marcas de límite y caché de tokens del helper. El App no necesita
ningún permiso más allá de Metadata (lectura).

```toml
[oidc.github]
host = "github.com"                     # el valor por defecto; un nombre de host en minúsculas
login_claim = "github_login"            # la claim con el login de GitHub del sujeto
roles = ["read", "triage", "write", "maintain", "admin"]   # por defecto; también roles propios
max_age_seconds = 5400                  # obligatorio, al menos 60
```

`[oidc.github]` exige `[postgres]` y un Meada compilado con `postgres`; a su lado, `[oidc]` no
necesita `identities_claim` ni `groups_claim`. `meada github-app roles --workspace <manifiesto>`
lista `GET /repos/{owner}/{repo}/collaborators?affiliation=all&per_page=100`, todas las páginas,
y escribe en Postgres, en una transacción, un conjunto de permisos que reemplaza al anterior:
cada colaborador listado con su `role_name`. Bajo `[refresh_lock]` el conjunto lleva el
`MEADA_REFRESH_FENCE` del job, y un conjunto con un fence mayor ya escrito rechaza la ejecución.
Un 403 o 429 pasa a la siguiente instalación; con todas limitadas, en un 5xx, un timeout o un
encabezado `Link` que sale del listado del repositorio, no se escribe nada y la salida es 1. Un
404, o un owner que ninguna instalación cubre, da un aviso y ninguna fila.

Con `source = "store"`, toda adición cuyo `added_by` no tiene un rol de `roles` queda fuera: el
comando escribe `<index_dir>/workspace/excluded` (el id de la versión, luego las identidades
excluidas, una por línea), y `meada index`, `status`, `export` y `snapshot publish` la tratan como
ausente del workspace. Sin ese archivo, o con uno de otra versión, se niegan y nombran `meada
github-app roles`; una verificación que falló no deja archivo. `meada workspace show --pulled`
imprime la versión descargada sin las exclusiones, para el script de fetch. El refresh del job,
bajo `meada snapshot lock`, queda: `workspace pull`, `github-app roles`, el fetch de lo que
`show --pulled` lista, `index --workspace` y `snapshot publish`; un refresh que se detiene en la
verificación no publica nada.

### El grant en la admisión

Los nodos y la consola con `[oidc.github]` leen cada minuto el conjunto de permisos más nuevo, y
cargan las filas cuyo rol está en `roles` solo cuando el conjunto cambia; la admisión nunca espera
a Postgres. El login de un token es el valor de `login_claim` cuando es una cadena de 1 a 39
letras ASCII, dígitos, `-` y `_`, comparado en minúsculas; cualquier otro valor no da grant de
GitHub. Use una claim que el administrador del proveedor de identidad mapea desde un atributo
verificado, nunca una que el usuario edita. El grant de GitHub es lo que ese login lee en el
conjunto mientras su edad (el reloj de Postgres menos `finished`, más el tiempo monotónico del
proceso desde la lectura) es como mucho `max_age_seconds`; después, con el job detenido o Postgres
inalcanzable, queda vacío y el grant de las claims sigue valiendo. Nunca vale `*`, y un token de
máquina con `*` no depende de Postgres. La sesión de la consola muestra `login`, `github_host` y
`github_grant` (`current`, `expired` o `unread`), la verificación de token separa las identidades
en `sources` (`claims` y `github`) y la página de admisión muestra el conjunto cargado. Un nodo
que encuentra una versión de schema que no conoce lo dice una vez en stderr y trata a Postgres
como inalcanzable.
