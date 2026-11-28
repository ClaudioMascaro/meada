# Clientes HTTP da JVM

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Reconhece clientes Java imperativos e interfaces declarativas usadas para enviar requisições.

## Interface

Um leitor acompanha RestTemplate, WebClient, `java.net.http` e OkHttp. Outro registra os métodos
OpenFeign e Retrofit na definição da interface, onde o grafo consegue alcançar o consumidor.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

## Invariantes

Imports e tipos identificam o framework. No Retrofit, cada `create` pode fornecer uma base. Um
caminho relativo cuja base é desconhecida fica em aberto.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

Em `@FeignClient`, uma URL que começa com texto literal sem `://`, como `localhost:8080`, recebe o
prefixo `http://`.
[declarative.rs:schemed](./declarative.rs)

## Efeitos colaterais

Os leitores consultam os fatos e a configuração Spring indexada para montar os resultados em
memória.
[clients.rs:consumers](./clients.rs) [declarative.rs:consumers](./declarative.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::java jvmpkg::
```
