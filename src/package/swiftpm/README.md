# Pacotes no manifesto SwiftPM

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Lê a parte de `Package.swift` que identifica o pacote, seus produtos de biblioteca e dependências.

## Interface

`package` recebe os bytes do manifesto e devolve `Package` com produtos, alvos de biblioteca e
entradas `Dependency`, incluindo caminho ou URL e requisito de versão.
[manifest.rs:package](./manifest.rs) [manifest.rs:Package](./manifest.rs) [manifest.rs:Dependency](./manifest.rs)

## Invariantes

Uma dependência precisa declarar exatamente um entre `url` e `path`. Partes calculadas ficam
marcadas com `*` e com o indicador `computed`.
[manifest.rs:package](./manifest.rs) [manifest.rs:Dependency](./manifest.rs)

## Efeitos colaterais

O parser percorre a árvore local na ordem do código e mantém o primeiro nome encontrado em
`Package(name:)`. O resultado fica em memória.
[manifest.rs:package](./manifest.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- package::swiftpm swiftpkg::
```
