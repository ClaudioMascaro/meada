# Versiones de workspace y owners

## Importar y aplicar una versión

El workspace administrado separa las entradas de repositorios de los módulos locales.
Prepare un archivo con `[[repository]]`, usando las rutas de los mirrors del job, e importe
una versión inmutable:

```toml
[[repository]]
repo = "github.exemplo.internal/acme/api"
path = "/var/lib/meada/mirrors/api"
```

```sh
meada workspace import repositories.toml --message "Add API repository" --workspace /etc/meada/job.workspace.toml
meada workspace list --workspace /etc/meada/job.workspace.toml
meada workspace show <version> --workspace /etc/meada/job.workspace.toml
meada workspace apply <version> --workspace /etc/meada/job.workspace.toml
meada workspace pull --workspace /etc/meada/job.workspace.toml
```

El import valida las mismas entradas que el editor de la consola; reimportar el mismo contenido
conserva el id y los metadatos de la primera escritura. `apply` mueve el pointer del workspace
bajo el lock, sin publicar una generación. `pull` instala esa versión localmente; los comandos
siguientes la leen sin abrir conexiones durante la indexación. El job tiene su propio bootstrap,
`index_dir`, mirrors y `local_dir`; comparte con los nodos y la consola solo las secciones de
módulos que apuntan al mismo store y Valkey. Ejecute todo su refresh bajo un único lock, con
el fetch fuera de Meada:

```sh
meada snapshot lock --workspace /etc/meada/job.workspace.toml -- sh -ec '
  meada workspace pull --workspace /etc/meada/job.workspace.toml
  ./fetch-mirrors.sh
  meada index --workspace /etc/meada/job.workspace.toml
  meada snapshot publish --workspace /etc/meada/job.workspace.toml
'
```

## De la versión a la generación

La generación publicada registra la versión del workspace que la originó. Aplicar una versión
anterior prepara el siguiente refresh; los nodos siguen sirviendo la generación que guardan
hasta otro publish y pull. El editor permite validar, comparar, guardar una versión y
aplicarla por separado.

La imagen `ghcr.io/claudiomascaro/meada:<versión>-cloud` incluye el frontend en
`/usr/share/meada/ui`. Para instalar con Cargo, use `cargo install meada --features cloud`,
descargue `meada-ui-<versión>.tar.gz` de la misma release, extráigalo y apunte `[ui] dir`
al directorio extraído. La versión de `meada-ui.json` debe coincidir con el binario; un frontend
de otra versión impide el inicio. Los perfiles de consola de [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) y
[Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) están deshabilitados por defecto y tienen bootstrap, admisión y
almacenamiento propios.

## Ownership por equipo

!!! warning "Planificado: S3"
    [S3, #294](https://github.com/ClaudioMascaro/meada/issues/294) define ownership y autorización por equipo. Las versiones y el editor anteriores ya están entregados en S2; las reglas siguientes aún no están disponibles.

El `owner` de una entrada ya existe como metadato (#322): una cadena de 1 a 256 caracteres, sin carácter de control ni espacio en blanco en los extremos, comparada exactamente, con mayúsculas y minúsculas. `meada workspace import` lo guarda, `show` lo imprime justo después de `path`, `pull` lo escribe y cambia el id de la versión; una entrada sin él conserva su id. La indexación y los ids de bundle y de generación nunca lo leen, y las entradas `[[repository]]` del propio manifest de bootstrap lo rechazan, citando `source = "store"`. Ninguna regla lo lee todavía para decidir quién puede escribir.

Una entrada podrá tener un `owner`, el nombre exacto de un grupo OIDC. El operador global delegará la primera entrada; el operador de equipo podrá modificar solo entradas de sus grupos que su grant permita leer. El viewer consultará lo que su grant admita. Ownership nunca ampliará el grant.

Asignar, sustituir **o borrar** un owner será `owner_change`, reservado al operador global. Guardar una propuesta creará una versión inmutable; aplicar la versión y publicar la generación seguirán siendo acciones distintas. Aplicar solo cambios de owners no requerirá indexar el código otra vez.

El diagrama distingue el camino entregado (líneas continuas) de la autorización planificada (líneas discontinuas). [S5](named-workspaces.md) añadirá workspaces con nombre sobre la indexación compartida.

<!-- meada-graph:workspaces -->
