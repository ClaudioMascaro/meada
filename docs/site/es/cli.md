# Uso

En la raíz del repositorio:

- `meada index` arma el índice, en `.meada/`, del repositorio y de los pares;
- `meada status` muestra el conjunto de repositorios, el estado de cada índice y los pares que
  no se encontraron; bajo cada repositorio, los recursos sin enlace y los `via` que no
  corresponden a ningún símbolo, y cada archivo fuente que un language pack descartó por pasar de un
  límite (`too broad`, en inglés, por el presupuesto de nombres cualificados, o un proyecto que da
  demasiados usings a sus archivos);
- `meada export` imprime todo el índice como JSON ordenado, útil para comparar dos ejecuciones
  con `diff`;
- `meada decide` resuelve enlaces ambiguos y URLs dinámicas, ver Decisiones;
- `meada mcp` sirve las [herramientas MCP](mcp.md).

El export empieza por tres campos que dicen de dónde viene. `format` nombra el formato del
export (1 hoy): sube cuando quien lee el formato anterior lo leería mal (un campo renombrado o
eliminado, un significado cambiado), nunca por un campo opcional añadido, y no es la versión del
esquema del índice. `commit` es el commit al que apuntaba `HEAD` cuando se leyó el índice, y
`dirty` dice si el checkout difería de él (archivos cambiados, eliminados o sin seguimiento; los
ignorados, como `.meada/`, no cuentan). Los dos desaparecen juntos fuera de git, antes del primer
commit y cuando `HEAD` o los archivos cambian durante la actualización. En modo servidor vienen de
lo que registró `meada index --workspace`, o de la generación, nunca del `HEAD` actual del espejo,
y allí `dirty` es siempre `false`.

Meada solo escribe dentro de `.meada/`, que se ignora a sí mismo en git. Toda consulta primero
actualiza el índice con lo que cambió en los archivos, incluso lo que aún no se hizo commit. Los
mensajes de la línea de comandos siguen `LANG` (portugués, inglés o español); los del MCP son siempre en
inglés.

### De una carga de trabajo al código

Con los repositorios de aplicación y despliegue en el mismo workspace, busque la imagen
normalizada, lea quién la construye y recorra el código que ejecuta la carga con las herramientas MCP:

```text
search(query="ghcr.io/acme/orders", kind="image")
context(symbol="image::ghcr.io/acme/orders")
context(symbol="Deployment.apps/shop/orders")
trace(from="Deployment.apps/shop/orders", direction="down")
trace(from="orders.py::orders", direction="up")
```

Si dos overlays usan la misma carga e imagen, la exportación conserva un enlace por raíz.
En consultas IaC, añada la raíz a la dirección, por ejemplo `Pod/common (root overlays/right)`.

Compose con `build: ./src` también llega al código sin publicar una imagen. `status` pregunta por
las imágenes huérfanas en el ámbito visible por registro y namespace de la imagen (`ghcr.io/acme`);
un constructor sin ejecutor en su grupo recibe `no runner in the workspace` en las consultas. El
console muestra las imágenes y sus ubicaciones en contratos, y las rutas directas en la vista del
código de las cargas. Consulte [Capacidades](capabilities.md).
