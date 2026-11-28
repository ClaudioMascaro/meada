# Rotas REST do API Gateway

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Associa métodos públicos e integrações declarados em Terraform aos caminhos de uma API REST da
AWS.

## Interface

`rest_routes` cruza métodos com integrações AWS e AWS_PROXY usando os blocos já avaliados.
[rest.rs:rest_routes](./rest.rs)

## Invariantes

A API, o recurso e o método público devem coincidir. O caminho é descartado se um ancestral
pertence a outra API, se há ciclo ou se um segmento contém `/`.
[rest.rs:rest_identity](./rest.rs) [rest.rs:rest_path](./rest.rs) [rest.rs:rest_routes](./rest.rs)

A busca recursiva por um atributo declarado também para sem resultado quando excede a profundidade
máxima de avaliação.
[rest.rs:declared_attribute](./rest.rs)

## Efeitos colaterais

As rotas aceitas são adicionadas ao resultado do avaliador, com o alvo e a indicação de que
valores podem ser sobrescritos.
[rest.rs:rest_routes](./rest.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- iac_http_routes:: http_iac::
```
