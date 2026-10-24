# Configuración

Cada repositorio puede tener un `meada.toml` en la raíz, versionado en git, que dice con quién
conversa. Quien consume una ruta nombra al par:

```toml
[[peer]]
repo = "example.com/acme/api"            # remoto git de origin, sin esquema ni .git
base_url_env = ["API_URL"]               # variable de la que el cliente lee la URL base
# path = "../api"                        # opcional: pista de dónde está el clon
```

Quien sirve la ruta declara los hosts en los que responde:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # opcional: lo que el gateway pone delante de las rutas
strip = "/v1"                            # opcional: lo que les quita del comienzo
```

Con `prefix` y `strip`, una llamada a `/shop/orders` llega a la ruta `/v1/orders` del código, en
el nivel `inferred` y con el motivo `gateway prefix /shop, gateway strip /v1`. Un segmento de
versión solo se quita cuando lo escribes en `strip`, y quien llama a la ruta del código sigue
enlazando como antes.

Un recurso compartido que ningún código nombra, como una tabla o un bucket, entra como
`[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # identificador en minúsculas
id = "orders"
role = "write"                           # read, write o read-write
via = ["src/orders.ts::cancelOrder"]     # opcional: los símbolos que hacen el acceso
```

Cada escritor enlaza con cada lector del mismo recurso en otro repositorio, en el nivel
`declared`. Sin `via`, o si ningún `via` corresponde a un símbolo, el lado es el repositorio
entero.

Meada encuentra los pares por su remoto git, aunque el clon esté en otra ruta en cada máquina.
Una clave desconocida o mal escrita da un error con el archivo, la línea y la clave. Sin
`meada.toml`, el repositorio no declara ningún par.
