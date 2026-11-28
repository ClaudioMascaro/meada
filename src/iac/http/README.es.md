# Rutas REST de API Gateway

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Relaciona métodos públicos e integraciones declarados en Terraform con las rutas de una API REST
de AWS.

## Interfaz

`rest_routes` cruza métodos con integraciones AWS y AWS_PROXY a partir de bloques ya evaluados.
[rest.rs:rest_routes](./rest.rs)

## Invariantes

La API, el recurso y el método público deben coincidir. Se descarta la ruta si un ancestro
pertenece a otra API, hay un ciclo o un segmento contiene `/`.
[rest.rs:rest_identity](./rest.rs) [rest.rs:rest_path](./rest.rs) [rest.rs:rest_routes](./rest.rs)

La búsqueda recursiva de un atributo declarado también termina sin resultado cuando supera la
profundidad máxima de evaluación.
[rest.rs:declared_attribute](./rest.rs)

## Efectos secundarios

Las rutas aceptadas se añaden al resultado del evaluador con su destino y una indicación de
valores que pueden sobrescribirse.
[rest.rs:rest_routes](./rest.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- iac_http_routes:: http_iac::
```
