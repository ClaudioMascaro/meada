# Varios nodos

Usa varios nodos para atender consultas en paralelo. Cada nodo mantiene su propia copia de los índices. Un proxy distribuye las peticiones; el transporte no necesita afinidad de sesión (*sticky session*). Los clientes del protocolo 2026-07-28 ni siquiera crean sesión.

El [ejemplo Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/README.es.md) incluye dos nodos, TLS y admisión por token en el proxy. El [chart Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/README.es.md) usa **Deployment**, con init, readiness y cierre gradual. El perfil StatefulSet siguiente requiere adaptar los manifests; el chart actual no lo genera.

## StatefulSet con S3 compartido

Este es un **perfil de referencia**, con los módulos existentes. StatefulSet da identidad estable a las réplicas y permite un volumen propio por pod. Meada no lo exige: el chart entregado usa Deployment y `emptyDir` por pod.

Aquí, un nodo Meada es una réplica del servidor ejecutada en un pod. Un nodo Kubernetes es la máquina que aloja pods. StatefulSet no garantiza que las réplicas ocupen máquinas distintas; configura anti-affinity o topology spread para distribuirlas entre workers y zonas, según la capacidad del cluster.

<!-- meada-graph:statefulset-s3 -->

El bucket y el prefijo S3 se comparten. Los archivos SQLite activos no. Cada réplica tiene su propio `local_dir`, con bundles extraídos, índices y puntero `current`. Un PVC por réplica puede conservar los archivos entre recreaciones; un volumen efímero exige repetir el pull inicial.

| Componente | Responsabilidad |
| --- | --- |
| Job de refresh | Obtiene mirrors, indexa localmente y publica, sin solaparse con otro job |
| S3 o store compatible | Guarda documentos y bundles inmutables bajo el mismo bucket/prefijo |
| Valkey | Coordina puntero de generación, retención, lock, fence y heartbeats |
| Init de cada pod | Ejecuta el primer `snapshot pull` antes de iniciar el servidor |
| Companion de cada pod | Repite el pull en el `local_dir` exclusivo de ese pod |
| Servidor MCP | Consulta índices locales con `--generations` |
| Proxy / Service | Distribuye peticiones entre réplicas listas |

Configura el mismo bucket, prefijo y dominio Valkey en el job y las réplicas. Asigna un `snapshot.node_name` único a cada nodo y a la consola; los nombres estables de los pods pueden cumplir esa función. El job conserva sus propios mirrors, `index_dir` y `local_dir`.

S3 se usa mediante la API de objetos, no como volumen montado. El backend de snapshot entregado usa un endpoint compatible con S3, con TLS fuera de loopback. No hay backend `file://`.

Consulta [snapshots](snapshots.md) para configurar el store, [generaciones](generations.md) para retención y [refresh](refresh.md) para lock y fence. Cualquier fallo de fetch o indexación aborta toda la publicación y conserva la generación actual ([#316](https://github.com/ClaudioMascaro/meada/issues/316)).

## Elegir cómo distribuir los índices

<!-- meada-graph:node-layout -->

**Cada nodo indexa sus mirrors.** El nodo obtiene el código y ejecuta `meada index --workspace` en un `index_dir` local. Este diseño no requiere módulos de nube. Los nodos pueden responder con commits distintos hasta que todos actualicen sus mirrors.

**Un job publica; los nodos reciben.** El job obtiene e indexa los mirrors. Después publica una generación, el conjunto de bundles con código e índices. Cada nodo descarga esa generación en su propio directorio y cambia el puntero local. [Snapshots](snapshots.md) explica la publicación y el pull.

Una reconstrucción renombra `index.db.new.<pid>` sobre `index.db` bajo `index.db.lock`. Los lectores ven el índice anterior o el nuevo completo. Sustituye un índice solo cuando ningún `meada index` lo esté escribiendo: un `index.db-journal` activo podría interpretarse como journal del archivo nuevo.

## Almacenamiento local y lectura

No compartas un `index_dir` activo entre escritor y lectores mediante NFS o EFS. [SQLite sobre sistemas de archivos de red tiene restricciones de fiabilidad](https://sqlite.org/useovernet.html). Cada nodo necesita almacenamiento local o un volumen que solo él escribe.

El servidor puede leer `index_dir` y mirrors de solo lectura, por ejemplo preparados por init o incluidos en la imagen. No crea archivos allí. Los mirrors de otro usuario se aceptan con `-c safe.directory`, solo para rutas del manifiesto. Esto exige git 2.38 o posterior; en Ubuntu 22.04 y RHEL 8 con git antiguo, configura `safe.directory` a nivel de sistema.

## Configurar proxy y disponibilidad

El proxy conserva `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` y todos los headers `Mcp-Param-*`. Puede enrutar por ellos, pero no alterarlos: rmcp rechaza diferencias entre headers y cuerpo (SEP-2243). Conserva también el header de respuesta `Meada-Generation`.

No añadas `Origin`: MCP responde 403 a peticiones con ese header, por lo que una página de navegador necesita un backend para llamarlo. El servidor MCP no comprueba `Host`. `--listen` acepta IP o `localhost`, sin resolver nombres.

Completa el init antes de admitir tráfico. Para el diseño local, usa una sonda TCP o `meada status --workspace <manifiesto>`: salida 0 indica que todos los repositorios pueden servirse; 1 indica fallo. Status espera hasta 50 ms por un commit y cuenta un índice bloqueado como listo. Para snapshots, usa [status con `--generations`](snapshots.md#disponibilidad).

`--listen` abre el puerto solo tras las comprobaciones iniciales. El 404 en `/` lista identidades y no es un health check.

## Ejecutar un escritor y detener réplicas

Evita jobs solapados, por ejemplo con `concurrencyPolicy: Forbid` en el CronJob, y usa el [lock de refresh](refresh.md). Configura `terminationGracePeriodSeconds` por encima del límite de 25 s de SIGTERM. El retraso de `preStop` debe dar tiempo al balanceador para retirar la réplica del tráfico.
