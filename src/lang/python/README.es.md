# Metadatos de proyectos Python

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Extrae de `pyproject.toml` la configuración que permite resolver importaciones, identificar
paquetes y reconocer scripts de entrada.

## Interfaz

`Pyproject` expone nombre, versión, requisitos, raíces de código, scripts y configuración de uv.
`source_roots` sitúa las raíces declaradas respecto del directorio del proyecto.
[pyproject.rs:Pyproject](./pyproject.rs) [pyproject.rs:source_roots](./pyproject.rs)

## Invariantes

Para el nombre, `[project]` tiene prioridad sobre Poetry. Una dependencia de uv debe indicar un
único origen reconocido. Se omiten listas de alternativas u orígenes múltiples.
[pyproject.rs:name](./pyproject.rs) [pyproject.rs:uv_sources](./pyproject.rs)

El lector considera los primeros 256 patrones de cada lista de globs del workspace uv.
[pyproject.rs:MAX_GLOBS](./pyproject.rs)

## Efectos secundarios

`parse` convierte los bytes recibidos en una tabla TOML y devuelve un error para UTF-8 o TOML no
válidos.
[pyproject.rs:parse](./pyproject.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::python pypi::
```
