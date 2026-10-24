# Instalación

Elige una de cinco formas.

- **Cargo**: `cargo install meada`.
- **Binario listo**: descarga el archivo de tu sistema desde los
  [releases de GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS y
  Windows, en x86_64 y arm64) y pon `meada` en el `PATH`.
- **Instalador** (Linux y macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.
- **npm**: `npx meada <comando>` descarga el binario listo de tu plataforma y lo ejecuta, sin
  instalar nada antes.
- **Contenedor**: `ghcr.io/claudiomascaro/meada:<versión>` para el build predeterminado, o
  `<versión>-cloud` con todos los módulos cloud, ambos para Linux amd64 y arm64. `latest` y
  `latest-cloud` siguen la última release estable; las prereleases solo tienen tags de versión.
  La base Debian bookworm-slim (git y certificados CA) solo se actualiza con una release de Meada.
  La imagen se ejecuta sin root y no define comando: pasa los argumentos explícitamente, por
  ejemplo `docker run --rm ghcr.io/claudiomascaro/meada:latest --version`. Para servir,
  monta el manifiesto, los espejos y los índices, y pasa
  `mcp --listen 0.0.0.0:7333 --workspace /etc/meada/meada.workspace.toml`, siguiendo
  [Modo servidor](server.md) y su configuración de admisión.

Para la consola, ejecuta `cargo install meada --features ui`, descarga
`meada-ui-<versión>.tar.gz` de la misma release y descomprímelo. Apunta `[ui] dir` en el
manifiesto al directorio `meada-ui-<versión>/` extraído. La consola rechaza iniciar con una UI
de otra versión e indica ambas versiones. La imagen `-cloud` ya incluye estos archivos en
`/usr/share/meada/ui`; la imagen predeterminada y los binarios listos siguen sin la UI.
Node solo es necesario para compilar el frontend desde los fuentes, nunca para instalar
mediante Cargo o ejecutar la consola.

Para indexar Java, solo se necesitan el binario de Meada y git. No hacen falta JDK, Maven ni Gradle; el análisis lee los archivos `.java` sin compilar ni ejecutar el proyecto.
