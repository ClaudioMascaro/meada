# Lectura de comandos de GitHub Actions

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Descompone el texto de un paso `run` para los extractores de paquetes e imágenes.

## Interfaz

`commands` recibe el script y una función para consultar el entorno del paso. Devuelve comandos
como listas de palabras con partes literales o abiertas.
[run.rs:commands](./run.rs) [run.rs:Part](./run.rs)

`environment_file` distingue un script que no menciona `GITHUB_ENV`, escrituras con nombres
visibles en `echo NOMBRE=...` y escrituras que pueden introducir nombres desconocidos.
[run.rs:environment_file](./run.rs)

## Invariantes

Un heredoc o un delimitador sin cerrar impide la lectura. Una variable solo se sustituye si su
valor es simple, cabe en el presupuesto y el script conserva su asignación.
[run.rs:commands](./run.rs) [run.rs:rebound](./run.rs)

## Efectos secundarios

El analizador consulta la función de entorno y mantiene el presupuesto y las palabras en memoria.
El texto del shell se trata como entrada de análisis estático.
[run.rs:commands](./run.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- iac::actions ci_run::
```
