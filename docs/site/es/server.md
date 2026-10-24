# Modo servidor

> **Atención.** En modo servidor Meada devuelve fragmentos de código fuente a quien logre
> conectarse, y por defecto no autentica a nadie: el proxy inverso del operador admite a los
> clientes (TLS, tokens, SSO, política de red); el [módulo OIDC](admission.md) es la alternativa opt-in. Sin host, el servidor escucha solo en el loopback; escuchar
> más allá es una elección explícita del operador, y un proxy mal configurado expone el código.
> El servidor MCP no revisa el encabezado `Host` (un proxy con nombre propio funciona) y responde 403 a
> cualquier solicitud con `Origin` (protección contra navegadores y DNS rebinding), en
> cualquier ruta y antes de cualquier otra respuesta. El proxy no debe exponer Meada más allá
> de los clientes que admite.


<!-- meada-graph:server-request -->

## Manifiesto y mirrors

El modo servidor ejecuta Meada una vez para una organización: `meada mcp --listen` sirve las
herramientas por HTTP a partir de espejos de los repositorios que lista un manifiesto de
workspace, con cada índice fuera del checkout. El manifiesto `meada.workspace.toml` vive en un
repositorio de operaciones, versionado, y solo se lee de la ruta que da `--workspace`:

```toml
index_dir = "/var/lib/meada"

[[repository]]
repo = "github.com/acme/web"
path = "/srv/mirrors/web"

[[repository]]
repo = "github.com/acme/api"
path = "/srv/mirrors/api"

[repository.serve]
hosts = ["api.acme.com"]
prefix = "/api"

[[repository.resource]]
kind = "postgres"
id = "orders"
role = "write"

[[repository.peer]]
repo = "github.com/acme/web"
base_url_env = "WEB_URL"
```

`index_dir` es obligatorio; `path` es absoluto o relativo a la carpeta del manifiesto. Los
bloques `serve`, `resource` y `peer` (el par no lleva `path`, pues el manifiesto ubica todos los
repositorios) son opcionales y valen para un espejo sin `meada.toml`: el `meada.toml` del propio
repositorio gana, completo, y los bloques del manifiesto se ignoran (`meada status` los nombra).
Un espejo es un checkout con árbol de trabajo; un clon bare no tiene qué indexar y se rechaza.

## Indexar y servir

- `meada index --workspace <manifiesto>` actualiza el índice de cada repositorio, en
  `<index_dir>/<identidad>/index.db`. Es el refresh del operador, ejecutado por un job de fetch
  o un webhook: Meada nunca hace fetch ni clona, y el servidor solo lee. Tras instalar un
  binario nuevo, ejecute `meada index --workspace` antes de reiniciar el servidor.
- `meada mcp --listen <dirección> --workspace <manifiesto>` sirve. La dirección es
  `host:puerto`, `:puerto` o `puerto`; el host es una IP o `localhost`, nunca resuelto, y el
  valor por defecto es el loopback. La primera línea de stderr es
  `listening on http://<host>:<puerto>`. SIGINT o SIGTERM detienen el servidor: deja de aceptar
  conexiones, termina las respuestas en curso y sale con código 0. La espera tiene un límite fijo de 25
  segundos, por debajo de los 30 s de gracia por defecto de Kubernetes: pasado el límite, las
  conexiones restantes se descartan y la salida sigue siendo 0.
- Cada repositorio es un endpoint, `http://<host>:<puerto>/<identidad>` (por ejemplo
  `http://meada.internal:7333/github.com/acme/web`); el cliente configura una URL por
  repositorio en que trabaja. Cualquier otra ruta responde 404 con las identidades servidas.
- El transporte HTTP es sin estado, con respuestas JSON: cualquiera de varios servidores sobre
  los mismos índices responde a cualquier llamada.
- El servidor lee el manifiesto y el `meada.toml` y el `meada.decisions.toml` de cada espejo una
  vez, al iniciar: cambiar esos archivos exige reiniciar el servidor. Un cambio de código en un
  espejo solo pide `meada index --workspace`; la siguiente llamada ya ve el índice nuevo.
- `context` imprime `index behind the mirror: run meada index --workspace` cuando el archivo de
  un fragmento cambió después de indexar. `trace` con `changes: uncommitted` se rechaza, pues el
  árbol de trabajo está en la máquina del cliente.
- `meada status --workspace <manifiesto>` muestra el estado de cada índice, y
  `meada export --workspace <manifiesto> --repo <identidad>` imprime el índice de un
  repositorio, por ejemplo para alimentar una base de conocimiento.
