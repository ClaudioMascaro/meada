# Origem de construtores Python

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Decide se o receptor de uma chamada Python pode ser atribuído a uma construção externa
identificável.

## Interface

`Factories::read` segue o receptor e devolve `Construction`, com nome e argumento opcional. Admite
até um nível de fábrica externa.
[arguments.rs:Factories](./arguments.rs) [arguments.rs:read](./arguments.rs) [arguments.rs:Construction](./arguments.rs)

## Invariantes

Todas as alternativas precisam sustentar a construção. Um ramo opaco ou um ciclo impede atribuir o
receptor ao construtor conhecido de outro ramo.
[arguments.rs:values](./arguments.rs) [arguments.rs:read](./arguments.rs)

## Efeitos colaterais

O leitor guarda resultados e debita o orçamento por pergunta. Ao mudar de arquivo, limpa os caches
de resultados e restabelece o orçamento do arquivo.
[arguments.rs:read](./arguments.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- names::python aws_sdk::
```
