# Módulos Terraform mapeados

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Interpreta chamadas de módulos conhecidos ou mapeados pelo repositório como recursos provisionados
e cargas de trabalho.

## Interface

`mapped_module` avalia a entrada na instância que fez a chamada. `module_workload` usa o
mapeamento para selecionar Lambda ou ECS.
[modules.rs:mapped_module](./modules.rs) [modules.rs:module_workload](./modules.rs)

## Invariantes

As condições de criação precisam permitir o recurso. Um nome gerado por prefixo impede identificar
o nome físico provisionado.
[modules.rs:module_created](./modules.rs) [modules.rs:mapped_module](./modules.rs)

Um argumento booleano ausente usa o padrão do módulo. Um argumento presente que não possa ser
avaliado como booleano deixa a condição desconhecida.
[modules.rs:module_bool](./modules.rs)

## Efeitos colaterais

O avaliador atualiza a projeção do nó, o nome ou padrão e as origens que podem sobrescrevê-lo,
dentro do orçamento da avaliação.
[modules.rs:mapped_module](./modules.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- custom_iac::
```
