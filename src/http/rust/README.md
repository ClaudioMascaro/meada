# Rotas e bibliotecas HTTP em Rust

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Reconhece rotas axum e descreve os construtores e encadeamentos dos frameworks Rust que a extração
HTTP aceita.

## Interface

`providers` percorre `Router::route` e os métodos que ligam verbos a handlers. As tabelas
`FRAMEWORKS` e `CLIENT_LIBRARIES` identificam objetos de servidor e clientes reqwest/ureq.
[axum.rs:providers](./axum.rs) [frameworks.rs:FRAMEWORKS](./frameworks.rs) [frameworks.rs:CLIENT_LIBRARIES](./frameworks.rs)

## Invariantes

Uma cadeia conserva a identidade do receptor apenas pelos métodos registrados para o objeto de
origem.
[frameworks.rs:passes_on](./frameworks.rs)

O leitor de axum limita a 64 passos a cadeia de métodos usada para identificar a rota.
[axum.rs:METHOD_CHAIN](./axum.rs)

## Efeitos colaterais

A extração monta em memória as rotas e seus handlers a partir dos fatos de código.
[axum.rs:providers](./axum.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::rust urls::
```
