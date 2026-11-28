# Compilación del sitio

Generación y validación del sitio multilingüe de Meada.

## Interfaz

Los hooks de MkDocs validan los grafos, generan SVG y expanden los marcadores de las páginas. El verificador lee el HTML generado y los enlaces de origen. `[hooks.py:on_pre_build]` `[hooks.py:on_page_markdown]` `[check.py:source_links_check]`

## Invariantes

El bundle reúne las licencias de los paquetes incluidos y la compilación falla si falta un aviso de licencia. `[build.mjs:notices]`

## Efectos secundarios

La generación escribe SVG, JavaScript, CSS y avisos de terceros en los recursos del sitio. Se eliminan los SVG generados que salen del conjunto de diagramas. `[hooks.py:on_pre_build]` `[build.mjs:writeFile]`

## Verificación

Desde la raíz del repositorio, ejecuta `mkdocs build --strict && python scripts/docs/check.py`.
