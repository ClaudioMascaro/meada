# Módulos e valores Swift

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Relaciona os arquivos Swift aos alvos de SwiftPM e extrai valores escritos em expressões e
chamadas.

## Interface

`targets` lê `Package.swift` pela gramática Swift. `value` interpreta expressões e `fill` registra
argumentos e receptor em uma chamada.
[manifest.rs:targets](./manifest.rs) [values.rs:value](./values.rs) [values.rs:fill](./values.rs)

## Invariantes

O nome de um alvo precisa ser literal. Um caminho explícito calculado, absoluto ou com `..` exclui
o alvo. Declarações dentro de `dependencies` não criam alvos.
[manifest.rs:targets](./manifest.rs)

A leitura de valores limita a profundidade de expressões a 32. Uma expressão mais profunda deixa a
parte excedente desconhecida.
[values.rs:VALUE_DEPTH](./values.rs)

## Efeitos colaterais

O parser devolve os alvos ordenados por diretório e nome. A extração de valores preenche o
registro da chamada em memória.
[manifest.rs:targets](./manifest.rs) [values.rs:fill](./values.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::swift swiftpkg::
```
