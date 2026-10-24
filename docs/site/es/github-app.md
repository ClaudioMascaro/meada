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
