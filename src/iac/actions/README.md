# Leitura dos comandos de GitHub Actions

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Decompõe o texto de uma etapa `run` para os extratores de pacotes e imagens.

## Interface

`commands` recebe o script e uma função para consultar o ambiente da etapa. Retorna comandos como
listas de palavras com partes literais ou em aberto.
[run.rs:commands](./run.rs) [run.rs:Part](./run.rs)

`environment_file` distingue um script que não menciona `GITHUB_ENV`, um que revela os nomes
escritos com `echo NOME=...` e um que pode gravar nomes desconhecidos.
[run.rs:environment_file](./run.rs)

## Invariantes

Um heredoc ou delimitador sem fechamento impede a leitura. Uma variável só é substituída quando o
valor é simples, cabe no orçamento e o script preserva sua atribuição.
[run.rs:commands](./run.rs) [run.rs:rebound](./run.rs)

## Efeitos colaterais

O analisador consulta a função de ambiente e mantém o orçamento e as palavras em memória. O texto
do shell é tratado como entrada de análise estática.
[run.rs:commands](./run.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- iac::actions ci_run::
```
