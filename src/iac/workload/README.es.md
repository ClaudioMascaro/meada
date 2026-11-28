# Contenedores ECS en Terraform

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Extrae imágenes y entradas de entorno de las definiciones de contenedores ECS.

## Interfaz

`ecs_workload` lee `container_definitions` desde `jsonencode` o JSON evaluado. `ecs_module` lee el
mapa del módulo ECS indexado por nombre de contenedor.
[ecs.rs:ecs_workload](./ecs.rs) [ecs.rs:ecs_module](./ecs.rs)

## Invariantes

Un entorno incompleto o que excede el presupuesto invalida el conjunto de entradas reunidas. Los
secretos conservan solo una referencia censurada. Las imágenes son independientes de esa
invalidación.
[ecs.rs:unread](./ecs.rs) [ecs.rs:ecs_environment](./ecs.rs)

## Efectos secundarios

Tras descontar el coste, el evaluador sustituye las imágenes y las entradas de entorno del nodo en
memoria.
[ecs.rs:ecs_workload](./ecs.rs) [ecs.rs:ecs_module](./ecs.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- aws_workloads::
```
