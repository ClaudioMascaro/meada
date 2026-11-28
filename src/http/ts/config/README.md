# Inclusões de configuração Backstage

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Expande `$include` e `$env` para que as URLs possam ser lidas a partir dos arquivos de
configuração.

## Interface

`tree` recebe os bytes da raiz, a função que fornece os arquivos do repositório, os caminhos
observados e o orçamento compartilhado. Devolve a árvore e os motivos das inclusões recusadas.
[includes.rs:tree](./includes.rs) [includes.rs:Reason](./includes.rs)

## Invariantes

Apenas arquivos JSON ou YAML dentro do repositório podem ser incluídos. A expansão recusa ciclos e
limita a profundidade a oito e as inclusões a 64 por raiz.
[includes.rs:tree](./includes.rs) [includes.rs:DEPTH](./includes.rs) [includes.rs:INCLUDES](./includes.rs)

A expansão também debita os bytes incorporados por raiz e pela leitura compartilhada entre raízes.
Incluir o mesmo arquivo duas vezes consome orçamento nas duas ocorrências.
[includes.rs:GRAFTED_MAX](./includes.rs) [includes.rs:READING_MAX](./includes.rs)

## Efeitos colaterais

O leitor pede os arquivos à função fornecida e debita as leituras no orçamento recebido. `$env`
preserva o nome da variável.
[includes.rs:tree](./includes.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::ts::config urls:: http_bases::
```
