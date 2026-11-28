# Metadados de projetos Python

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Extrai de `pyproject.toml` a configuração usada para resolver imports, identificar pacotes e
reconhecer scripts de entrada.

## Interface

`Pyproject` expõe nome, versão, requisitos, raízes de código, scripts e configuração uv.
`source_roots` situa as raízes declaradas em relação ao diretório do projeto.
[pyproject.rs:Pyproject](./pyproject.rs) [pyproject.rs:source_roots](./pyproject.rs)

## Invariantes

Para o nome, `[project]` tem precedência sobre Poetry. Uma dependência uv precisa indicar uma
única origem reconhecida. Listas de alternativas ou múltiplas origens são ignoradas.
[pyproject.rs:name](./pyproject.rs) [pyproject.rs:uv_sources](./pyproject.rs)

O leitor considera os primeiros 256 padrões de cada lista de globs do workspace uv.
[pyproject.rs:MAX_GLOBS](./pyproject.rs)

## Efeitos colaterais

`parse` converte os bytes fornecidos em uma tabela TOML e devolve erro para UTF-8 ou TOML
inválido.
[pyproject.rs:parse](./pyproject.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::python pypi::
```
