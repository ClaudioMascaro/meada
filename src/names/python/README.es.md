# Origen de constructores Python

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Determina si el receptor de una llamada Python se puede atribuir a una construcción externa
identificable.

## Interfaz

`Factories::read` sigue al receptor y devuelve `Construction` con un nombre y un argumento
opcional. Admite hasta un nivel de fábrica externa.
[arguments.rs:Factories](./arguments.rs) [arguments.rs:read](./arguments.rs) [arguments.rs:Construction](./arguments.rs)

## Invariantes

Todas las alternativas deben respaldar la construcción. Una rama opaca o un ciclo impide atribuir
el receptor al constructor conocido de otra rama.
[arguments.rs:values](./arguments.rs) [arguments.rs:read](./arguments.rs)

## Efectos secundarios

El lector guarda resultados y descuenta trabajo por consulta. Al cambiar de archivo limpia las
cachés de resultados y restablece el presupuesto del archivo.
[arguments.rs:read](./arguments.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- names::python aws_sdk::
```
