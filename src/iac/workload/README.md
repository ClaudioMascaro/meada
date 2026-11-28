# Containers ECS em Terraform

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Extrai imagens e entradas de ambiente das definições de containers ECS.

## Interface

`ecs_workload` interpreta `container_definitions` a partir de `jsonencode` ou de JSON avaliado.
`ecs_module` lê o mapa indexado por nome de container do módulo ECS.
[ecs.rs:ecs_workload](./ecs.rs) [ecs.rs:ecs_module](./ecs.rs)

## Invariantes

Um ambiente incompleto ou acima do orçamento invalida o conjunto de entradas acumulado. Segredos
conservam apenas a referência redigida. As imagens são independentes dessa invalidação.
[ecs.rs:unread](./ecs.rs) [ecs.rs:ecs_environment](./ecs.rs)

## Efeitos colaterais

Após debitar o custo, o avaliador substitui as imagens e as entradas de ambiente do nó em memória.
[ecs.rs:ecs_workload](./ecs.rs) [ecs.rs:ecs_module](./ecs.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- aws_workloads::
```
