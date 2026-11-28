# Referências, receptores e usos em TypeScript e JavaScript

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

O pack registra a origem dos receptores, referências fora de chamadas e os usos de resultados que
revelam a estrutura dos dados.

## Interface

`References` reúne nomes usados como tipos, valores e elementos JSX. `inferred` reconhece
`z.infer<typeof S>`. `Uses::of` acompanha o que a função faz com o resultado de cada chamada.
[references.rs:References](./references.rs) [types.rs:inferred](./types.rs) [uses.rs:of](./uses.rs)

O pack registra a classe do receptor de uma chamada a partir de parâmetros tipados, variáveis
locais, campos de instância e resultados declarados. O valor contido em `Promise<T>` só fornece o
tipo após `await`, quando `Promise` identifica o tipo global.
[receivers.rs:local](./receivers.rs) [receivers.rs:definition](./receivers.rs) [receivers.rs:result](./receivers.rs)

## Invariantes

O orçamento de uso é compartilhado pelas chamadas de um arquivo. Quando se esgota, os usos
seguintes ficam vazios. Um tipo genérico oculto por classe, interface ou alias fica desconhecido.
[uses.rs:USES_BUDGET](./uses.rs) [types.rs:reference](./types.rs)

Construtores e namespaces globais, como `Map` e `JSON`, só contam onde nenhuma declaração ou
atribuição oculta o nome. A busca por um parâmetro de tipo que oculte uma referência percorre até
64 ancestrais.
[globals.rs:constructor](./globals.rs) [globals.rs:namespace](./globals.rs) [globals.rs:Bindings](./globals.rs) [types.rs:HIDING_CLIMB](./types.rs)

## Efeitos colaterais

A coleta deduplica as referências e mantém um cache dos usos por escopo enquanto a árvore
sintática está disponível.
[references.rs:push](./references.rs) [uses.rs:Uses](./uses.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::ts uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases::
```
