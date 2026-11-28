# Clientes HTTP Python e Rust

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Compartilha a leitura de chamadas HTTP entre requests, httpx, aiohttp e as tabelas Rust de reqwest
e ureq.

## Interface

`consumers` recebe fatos de código, identifica funções do módulo ou métodos de um cliente e
expande wrappers nos locais de chamada.
[clients.rs:consumers](./clients.rs)

## Invariantes

A origem do receptor identifica a biblioteca. No aiohttp um caminho iniciado por `/` substitui o
caminho da base mesmo se ela for desconhecida. Uma referência relativa que depende dessa base fica
em aberto.
[clients.rs:consumers](./clients.rs)

## Efeitos colaterais

O resultado é um vetor de consumidores construído em memória a partir dos fatos de código.
[clients.rs:consumers](./clients.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::python urls::
```
