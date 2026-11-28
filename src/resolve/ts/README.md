# Campos e métodos de objetos em TypeScript

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

O resolvedor identifica quais valores de campos e quais métodos de objetos literais ainda podem
ser usados no ponto de leitura.

## Interface

`ts_construction` resolve a classe no local do `new`. `ts_literal_method` procura o método de um
objeto literal. `ts_field_argument` recupera o argumento passado a um campo na construção exata,
inclusive por uma fábrica. `ts_record` seleciona a propriedade pela chave literal ou reúne os
valores possíveis quando a chave é dinâmica.
[fields.rs:ts_construction](./fields.rs) [fields.rs:ts_literal_method](./fields.rs) [fields.rs:ts_field_argument](./fields.rs) [fields.rs:ts_record](./fields.rs)

## Invariantes

Uma escrita além da inicialização ou um índice incompleto pode invalidar o campo. Leituras de
produção usam o índice de escritas de produção. Leituras em testes também consideram escritas de
testes. Na propagação de um argumento de construtor, uma subclasse conhecida ou um parâmetro
reatribuído deixa o valor desconhecido.
[fields.rs:ts_field_written](./fields.rs) [fields.rs:ts_field_argument](./fields.rs)

## Efeitos colaterais

`Writes::index` percorre os fatos do repositório e cria índices em memória por proprietário e nome
de campo. Conserva a localização de uma escrita única para distinguir a inicialização das demais
escritas.
[fields.rs:Writes](./fields.rs) [fields.rs:index](./fields.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- ts_receivers:: field_flow:: http_bases::
```
