# Snapshots

**Snapshot** (feature `snapshot`, `cargo install meada --features snapshot`). Un job indexa y publica; los nodos reciben la generación preparada. Explora la secuencia y configura el store para publicar y recibir los bundles.


<!-- meada-graph:snapshot-lifecycle -->

<details markdown="1" class="reference-details">
<summary>Validación antes de publicar y formato de la generación</summary>

**Snapshot** (feature `snapshot`, `cargo install meada --features snapshot`). Un job indexa y
publica; los nodos reciben el resultado entero, en vez de indexar cada uno sus propios espejos.
Después de `meada index --workspace`, que sigue sin red, el job ejecuta
`meada snapshot publish --workspace <manifiesto>`. Comprueba, solo leyendo, que cada índice es
igual a su espejo y que cada espejo está limpio (`git status` no lista nada, ni un archivo
ignorado), y rechaza listando cada repositorio y el comando `meada index --workspace` que lo
resuelve. Luego sube una generación: un bundle por repositorio (un tar del espejo, `.git`
incluido, y de su `index.db`), un documento que los lista con su tamaño y su hash BLAKE3, y al
final el puntero `current`. Su única línea en stdout es el id de la generación. Un bundle o una
generación ya guardados no se suben de nuevo, y nada se escribe en la máquina que publica.

</details>

## Configurar el store

```toml
[snapshot]
endpoint = "https://s3.us-east-1.amazonaws.com"   # cualquier endpoint compatible con S3
region = "us-east-1"
bucket = "ejemplo-meada"
prefix = "prod"                                    # opcional
path_style = false                                 # true en la mayoría de los stores self-hosted
# ca_file = "ca.pem"                              # CA privada opcional, relativa al manifiesto
credentials = "static"                             # o "web-identity", "pod-identity", "instance"
access_key_id_env = "AWS_ACCESS_KEY_ID"            # valores por defecto; o las claves *_file
secret_access_key_env = "AWS_SECRET_ACCESS_KEY"
session_token_env = "AWS_SESSION_TOKEN"            # opcional
local_dir = "/var/lib/meada/generations"           # para el pull y el servicio de generaciones
```

## Credenciales y transporte

<details markdown="1" class="reference-details">
<summary>Elegir la fuente de credenciales</summary>

El publish solo se conecta al `endpoint`, con TLS fuera de loopback, y al endpoint de credencial
de la fuente elegida; ninguna petición sigue una redirección ni pasa por un proxy. Las
credenciales vienen de una sola fuente, la que nombra `credentials`: `static` lee la clave y el
secreto de las variables o los archivos que nombra la sección (nunca del manifiesto);
`web-identity` (IRSA) lee `AWS_WEB_IDENTITY_TOKEN_FILE` y `AWS_ROLE_ARN` e intercambia el token
con STS en la `region`, por https; `pod-identity` (EKS Pod Identity) llama al agente en
`AWS_CONTAINER_CREDENTIALS_FULL_URI`, solo `http://169.254.170.23` o `http://[fd00:ec2::23]`,
con el token de `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`; `instance` usa el servicio de metadatos
de la instancia (IMDSv2, solo por IPv4, `http://169.254.169.254`), y solo cuando la sección lo indica. Las
credenciales quedan solo en memoria, y ningún secreto aparece en un error.

</details>

`ca_file` añade certificados PEM a las raíces de confianza del sistema. El archivo tiene como
máximo 1 MiB; los certificados inválidos y un archivo vacío se rechazan, nombrando el archivo.

## Recibir una generación

En cada nodo, `meada snapshot pull --workspace <manifiesto>` lee el puntero `current` del store
y, cuando nombra una generación que el nodo no tiene, descarga sus bundles en `local_dir`,
comprueba el tamaño y el hash BLAKE3 de cada uno contra el documento, los extrae y cambia el
puntero del nodo por rename. La única línea en stdout es el id de la generación actual.
`--every <segundos>` repite el pull (y solo imprime cuando el puntero cambia); un segundo pull en
el mismo `local_dir` sale con 0 avisando que ya hay uno en curso. El nodo guarda la generación
actual y la anterior y borra las más antiguas.

## Validar los bundles

<details markdown="1" class="reference-details">
<summary>Reglas de validación y aislamiento de bundles</summary>

Todo lo que viene del store se trata como entrada hostil: ids e identidades se comprueban antes
de volverse rutas; el pull rechaza en el archivo una ruta absoluta, un `..`, un enlace que sale
del repositorio, una entrada escrita a través de un enlace simbólico y dispositivos o FIFOs, con
techos de bytes y de entradas por generación. El `.git/config` extraído se reemplaza por uno
mínimo que Meada escribe (solo `[core]` y el `origin` de la identidad), `.git/hooks` se borra, y
el `git` de Meada corre en esos espejos sin la configuración del sistema ni la global. Un pull
que falla en cualquier paso deja el puntero y las generaciones guardadas como estaban.

</details>

```
<local_dir>/
  current                     # "<id de generación> <fence>", fence 0 sin el bloqueo de refresh
  pull.lock
  <id de generación>/
    generation.json           # el documento, como está guardado
    <id de bundle>/index.db
    <id de bundle>/mirror/    # el checkout, .git incluido
```

## Servir sin indexar

Para servir lo que el nodo recibió, `meada mcp --listen <dirección> --workspace <manifiesto>
--generations`. El manifiesto del nodo aporta solo las secciones de módulo (`[snapshot]`); los
repositorios son los que registró la generación actual, con los espejos e índices dentro de
`<local_dir>/<id de generación>/`. Cada llamada lee `<local_dir>/current` y abre el motor sobre
la generación que nombra: una llamada ya abierta termina en la generación en que empezó, la
siguiente ve la nueva, y `meada snapshot pull --every` cambia la generación sin reiniciar el
servidor (un repositorio nuevo en una generación nueva recibe su endpoint al instante). No se
escribe nada por llamada, los índices se abren solo para lectura y el servidor no abre ninguna
conexión con el store. Los nodos nunca ejecutan `meada index`: el job indexa y el nodo solo
recibe. Sin generación actual, o con un índice que no está en la versión de esquema de este
binario, el servidor se niega a arrancar y nombra `meada snapshot pull` o el repositorio y la
generación.

## Disponibilidad

`meada status --workspace <manifiesto> --generations` es la sonda de disponibilidad del nodo.
Imprime el id de la generación actual, los ids guardados y una línea
`store: reachable (current <id>)` o `store: unreachable (<motivo>)`. El código de salida
depende solo del estado local: 1 sin una generación actual servible (los problemas van a
stderr), 0 en caso contrario, con el store alcanzable o no, de modo que un store caído no saca
al nodo de la rotación. `meada export --workspace <manifiesto> --repo <identidad> --generations`
exporta del mismo modo.
