# Clientes HTTP Ktor

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Extrai consumidores de chamadas Kotlin a `HttpClient` do Ktor.

## Interface

`consumers` identifica o verbo importado, a origem do receptor e a URL passada pelo chamador.
Considera também a base de `defaultRequest`.
[client.rs:consumers](./client.rs)

## Invariantes

Um `request` sem atribuição de método usa GET. Uma atribuição condicional deixa o método
desconhecido. Uma URL definida apenas no bloco construtor fica sem resolução.
[client.rs:consumers](./client.rs)

## Efeitos colaterais

A leitura de valores injetados usa a configuração indexada do módulo e retorna os consumidores em
memória.
[client.rs:consumers](./client.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::kotlin kotlinpkg::
```
