# Ejemplos de mantenimiento

Dos ejecutables ayudan a revisar exportaciones y mantener el mapa del repositorio.

## Interfaz

`golden_diff` compara exportaciones por sección e identidad. `split_agents` divide un documento por encabezados de segundo nivel. `[golden_diff.rs:diff_docs]` `[split_agents.rs:split]`

## Invariantes

El divisor rechaza sobrescribir una parte existente con contenido distinto. La comparación `A...B` requiere que el ancestro común esté disponible en el clon. `[golden_diff.rs:resolve]` `[split_agents.rs:main]`

## Efectos secundarios

La comparación lee revisiones de Git. El divisor escribe el índice y los archivos de las secciones. `[golden_diff.rs:git]` `[split_agents.rs:main]`

## Verificación

Desde la raíz del repositorio, ejecuta `cargo test --examples`.
