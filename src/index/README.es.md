# Caché de valores del índice

Caché de valores que evita repetir el análisis de archivos sin cambios durante la actualización del índice.

## Interfaz

El índice abre su caché bajo demanda, consulta los hechos por ruta y clave, encola hechos nuevos y conserva las entradas necesarias después de la extracción. `[values.rs:beside]` `[values.rs:get]` `[values.rs:put]` `[values.rs:keep]`

## Invariantes

La clave incluye los bytes, la configuración del lenguaje, el pack y el build de Meada. Una caché ausente o ilegible da un fallo de caché y permite volver a analizar el archivo. `[values.rs:key]` `[values.rs:get]`

Una entrada que supera los 8 MiB de JSON descomprimido no se reutiliza. El índice vuelve a analizar el archivo cuando necesita esos valores. `[values.rs:ENTRY_BYTES]` `[values.rs:facts_of]`

## Efectos secundarios

Escribe y depura entradas comprimidas en `values.db`, junto al índice SQLite. Un índice en memoria no crea ese archivo. `[values.rs:FILE]` `[values.rs:beside]` `[values.rs:keep]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it freshness::a_refresh_over_the_values_cache`.
