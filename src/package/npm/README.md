# Registro de plugins npm no Backstage

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Identifica pacotes carregados como plugins num backend Backstage ou como features num backend de
teste.

## Interface

`added_plugin` devolve o pacote passado a `backend.add(import(...))`. `test_features` extrai os
imports da lista `features` entregue a `startTestBackend`.
[backstage.rs:added_plugin](./backstage.rs) [backstage.rs:test_features](./backstage.rs)

## Invariantes

O backend deve ter uma única origem constante ligada a `createBackend` do pacote esperado. Só um
`import()` literal identifica o plugin.
[backstage.rs:made_by](./backstage.rs) [backstage.rs:added_plugin](./backstage.rs)

`added_plugin` exige que o receptor de `.add` seja um nome simples e que a chamada receba
exatamente um argumento. Assim, `app.backend.add(...)` e uma chamada com argumentos extras ficam
de fora.
[backstage.rs:added_plugin](./backstage.rs)

## Efeitos colaterais

Os leitores consultam importações e argumentos indexados e devolvem os nomes encontrados em
memória.
[backstage.rs:added_plugin](./backstage.rs) [backstage.rs:test_features](./backstage.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- package::npm packages::
```
