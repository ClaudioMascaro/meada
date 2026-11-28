# Exemplos de manutenção

Dois executáveis auxiliam a revisão de exportações e a manutenção do mapa do repositório.

## Interface

`golden_diff` compara exportações por seção e identidade. `split_agents` separa um documento por títulos de segundo nível. `[golden_diff.rs:diff_docs]` `[split_agents.rs:split]`

## Invariantes

O divisor recusa sobrescrever uma parte cujo conteúdo difere da divisão proposta. A comparação `A...B` exige um ancestral comum disponível no clone. `[golden_diff.rs:resolve]` `[split_agents.rs:main]`

## Efeitos colaterais

O comparador lê revisões do Git. O divisor escreve o índice e os arquivos das seções. `[golden_diff.rs:git]` `[split_agents.rs:main]`

## Verificação

Na raiz do repositório, execute `cargo test --examples`.
