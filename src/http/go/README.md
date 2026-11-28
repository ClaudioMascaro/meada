# Clientes HTTP em Go

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Converte chamadas de `net/http` e Resty em consumidores HTTP.

## Interface

`consumers` recebe os fatos de código e devolve o método e o destino de cada chamada reconhecida.
`is_package_send` identifica envios feitos pelo pacote.
[clients.rs:consumers](./clients.rs) [clients.rs:is_package_send](./clients.rs)

## Invariantes

O receptor precisa estar ligado à biblioteca por importação, tipo declarado ou construção. A URL
base de Resty acompanha a instância, inclusive quando uma fábrica cria dois clientes.
[clients.rs:consumers](./clients.rs)

Ao determinar o cliente armazenado em um campo, uma leitura em produção descarta os valores
fornecidos apenas por testes. A leitura em arquivo de teste considera ambos. A busca por origens é
limitada a 64 passos e pode deixar o cliente desconhecido.
[clients.rs:held](./clients.rs) [clients.rs:CLIENT_DEPTH](./clients.rs) [../../resolve.rs:field_values](../../resolve.rs)

## Efeitos colaterais

A extração acumula consumidores em memória a partir das chamadas e das bases encontradas.
[clients.rs:consumers](./clients.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::go gopkg:: base_url::
```
