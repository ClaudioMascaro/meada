# Rotas e clientes HTTP em C#

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Identifica emissores de requisições HttpClient e Refit e rotas de Minimal APIs no ASP.NET Core.

## Interface

`providers` liga `MapGroup` e `MapMethods` a rotas. Os dois leitores `consumers` distinguem
chamadas de HttpClient e métodos declarados em interfaces Refit.
[minimal.rs:providers](./minimal.rs) [clients.rs:consumers](./clients.rs) [refit.rs:consumers](./refit.rs)

## Invariantes

No HttpClient um caminho iniciado por `/` substitui o caminho da base. No Refit a rota anotada
mantém o caminho da base. Uma lista parcialmente desconhecida de verbos em `MapMethods` impede o
registro.
[clients.rs:rebase](./clients.rs) [refit.rs:url](./refit.rs) [minimal.rs:methods](./minimal.rs)

Quando o grafo de grupos de Minimal APIs excede 4.096 nós, o leitor omite as rotas desse grafo
para evitar publicar uma lista incompleta de prefixos.
[minimal.rs:MINIMAL_NODES](./minimal.rs)

## Efeitos colaterais

`Reads` mantém em memória a configuração já indexada e carrega os valores sob demanda. A leitura
de uma variável de ambiente preserva seu nome.
[reads.rs:Reads](./reads.rs) [reads.rs:read](./reads.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::csharp nugetpkg::
```
