# Módulos, macros e valores Rust

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Reconstrói a árvore de módulos Cargo e os fatos sintáticos que ajudam a resolver valores e
referências no código Rust.

## Interface

Os leitores reconstroem módulos e `cfg`, reconhecem macros de itens que preservam os itens,
identificam menções e extraem valores, incluindo strings construídas por acréscimos sucessivos.
[crates.rs:Tree](./crates.rs) [cfg.rs:holds](./cfg.rs) [macros.rs:bodies](./macros.rs) [mentions.rs:mentions](./mentions.rs) [values.rs:Reader](./values.rs) [builders.rs:builder](./builders.rs)

## Invariantes

A configuração considera Linux x86_64 e as features padrão da crate. Predicados malformados ou
profundos demais resultam em falso. A busca dos módulos usa apenas os arquivos presentes no
inventário.
[cfg.rs:holds](./cfg.rs) [cfg.rs:defaults](./cfg.rs) [crates.rs:Tree](./crates.rs)

Os leitores de strings montadas por operações sucessivas compartilham um orçamento por arquivo.
Quando ele se esgota, as construções restantes ficam desconhecidas.
[builders.rs:BUILDER_BUDGET](./builders.rs) [builders.rs:builder](./builders.rs)

## Efeitos colaterais

`record` acrescenta aos fatos do arquivo as definições e invocações de macros relevantes para uma
nova análise dos itens.
[macros.rs:record](./macros.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::rust cargo::
```
