# Ferramentas de manutenção

Verificação local da branch, perfis de implantação e ferramentas usadas pelos testes.

## Interface

`scripts/check` seleciona verificações pelo escopo da mudança. O oráculo de expressões regulares recebe um vetor JSON e testa cada padrão com o motor de Go. `[check:main]` `[regexp-oracle.go:main]`

## Invariantes

O oráculo devolve dois resultados por padrão, na ordem de entrada. Uma compilação bem-sucedida produz uma string vazia. `[regexp-oracle.go:main]`

## Efeitos colaterais

O verificador executa processos de build e teste. O oráculo escreve em stdout ou no arquivo passado como segundo argumento. `[check:bounded]` `[regexp-oracle.go:main]`

## Componentes

- [Build do site](./docs/README.md)

## Verificação

Na raiz do repositório, execute `scripts/check --selftest`.
