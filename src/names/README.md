# Identificação de nomes externos

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Identifica bibliotecas e construtores pelos nomes que as importações de cada linguagem permitem
resolver.

## Interface

`imported` identifica o nome qualificado de uma importação Python. `factory` e
`constructor_argument` recuperam a origem externa do cliente TypeScript e do primeiro argumento
construído.
[python.rs:imported](./python.rs) [ts.rs:factory](./ts.rs) [ts.rs:constructor_argument](./ts.rs)

`factory_of` lê a construção de um valor TypeScript no escopo em que foi escrito. `field_factory`
identifica o cliente de um campo `private` ou `#campo` quando todas as atribuições não nulas
apontam para a mesma fábrica. Aceita a chamada de método em outro campo privado e interrompe
cadeias cíclicas.
[ts.rs:factory_of](./ts.rs) [ts.rs:field_factory](./ts.rs) [ts.rs:field_factory_in](./ts.rs)

- [Origem de construtores Python](./python/README.md)

## Invariantes

No TypeScript a fábrica precisa chegar a uma importação externa por associações constantes. O
argumento é examinado no escopo original antes da expansão.
[ts.rs:factory](./ts.rs) [ts.rs:constructor_argument](./ts.rs)

## Efeitos colaterais

A leitura das opções chama o leitor fornecido com os campos recuperados e devolve os resultados em
memória, preservando os locais de chamada.
[ts.rs:constructor_options](./ts.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- names:: aws_sdk:: gcp_sdk:: azure_sdk:: kafkajs_classes::
```
