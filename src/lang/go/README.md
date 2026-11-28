# Seleção de arquivos e valores Go

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Determina que arquivos e pacotes entram na análise Go e quais valores uma expressão fornece ao
grafo.

## Interface

`builds` e `listed` filtram arquivos e diretórios. `Modules` resolve imports por `go.mod`,
inclusive `replace`. `value` extrai literais, chamadas e partes de strings.
[build.rs:builds](./build.rs) [build.rs:listed](./build.rs) [modfile.rs:Modules](./modfile.rs) [values.rs:value](./values.rs)

`env_fields` guarda o nome da variável de ambiente de cada campo com tag `env` ou `envconfig`,
inclusive o caminho de campos em structs anônimas aninhadas. `fmt.Sprintf` aceita como formato uma
constante textual do arquivo quando o nome continua visível.
[values.rs:env_fields](./values.rs) [values.rs:env_tag](./values.rs) [values.rs:format](./values.rs)

## Invariantes

O alvo é sempre `linux/amd64` com `gc`. Uma restrição ilegível exclui o arquivo. `./...` deixa de
fora `testdata` e diretórios cujo nome começa por `.` ou `_`.
[build.rs:builds](./build.rs) [build.rs:listed](./build.rs)

O leitor rejeita uma restrição de build que atinge 64 níveis. Uma chamada adiada que possa alterar
o único resultado nomeado deixa seu valor de retorno desconhecido.
[build.rs:CONSTRAINT_DEPTH](./build.rs) [../go.rs:deferred_write](../go.rs)

## Efeitos colaterais

Os leitores montam a tabela de módulos a partir dos bytes de configuração fornecidos e avaliam as
expressões a partir da árvore sintática.
[modfile.rs:Modules](./modfile.rs) [values.rs:value](./values.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::go gopkg:: uses:: base_url:: kafka_env::
```
