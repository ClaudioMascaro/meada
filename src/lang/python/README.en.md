# Python project metadata

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Extracts the `pyproject.toml` settings used to resolve imports, identify packages and recognize
entry scripts.

## Interface

`Pyproject` exposes the name, version, requirements, source roots, scripts and uv configuration.
`source_roots` locates declared roots relative to the project directory.
[pyproject.rs:Pyproject](./pyproject.rs) [pyproject.rs:source_roots](./pyproject.rs)

## Invariants

For names, `[project]` takes precedence over Poetry. A uv dependency must specify exactly one
recognized source. Lists of alternatives or multiple sources are skipped.
[pyproject.rs:name](./pyproject.rs) [pyproject.rs:uv_sources](./pyproject.rs)

The reader considers the first 256 patterns in each uv workspace glob list.
[pyproject.rs:MAX_GLOBS](./pyproject.rs)

## Side effects

`parse` converts supplied bytes into a TOML table and returns an error for invalid UTF-8 or TOML.
[pyproject.rs:parse](./pyproject.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::python pypi::
```
