# Rotas e clientes HTTP em Swift

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Lê rotas Vapor e Hummingbird e chamadas de clientes Swift.

## Interface

`providers` usa um dialeto para ler grupos e handlers nos dois frameworks. `consumers` reconhece
URLSession, Alamofire e AsyncHTTPClient.
[vapor.rs:providers](./vapor.rs) [hummingbird.rs:HUMMINGBIRD](./hummingbird.rs) [client.rs:consumers](./client.rs)

## Invariantes

Os prefixos pertencem ao grupo que registra a rota. No lado cliente, a origem do receptor precisa
identificar a biblioteca.
[vapor.rs:providers](./vapor.rs) [client.rs:consumers](./client.rs)

A busca pela origem do cliente atravessa até oito níveis de nomes locais e constantes.
[client.rs:CLIENT_DEPTH](./client.rs)

## Efeitos colaterais

Os leitores percorrem os fatos Swift disponíveis e retornam rotas e consumidores em memória.
[vapor.rs:providers](./vapor.rs) [client.rs:consumers](./client.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::swift swiftpkg::
```
