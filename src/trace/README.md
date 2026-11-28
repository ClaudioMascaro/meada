# Impacto de mudanças no grafo

Cálculo dos arquivos, pacotes e lados de contrato afetados por uma mudança para `meada trace --affected`.

## Interface

A atribuição de lockfiles identifica dependências alteradas. A identificação das raízes relaciona os caminhos modificados a símbolos, locais de configuração, nós IaC e lados de documentos, e a resposta agrega os dependentes alcançados. Um lockfile em um subdiretório só pode cobrir manifestos alterados de membros dentro desse diretório. `[affected.rs:attribute]` `[affected.rs:roots]` `[affected.rs:answer]`

## Invariantes

Um caminho modificado sem raiz conhecida aparece em `open` com a razão. As listas da resposta têm ordem estável. `[affected.rs:roots]` `[affected.rs:answer]`

Cada arquivo alcançado recebe a classificação da aresta mais fraca do seu caminho mais forte (`certain`, `inferred`, `model` ou `ambiguous`). Uma execução obtida de um chart alcança apenas arquivos do pacote dono, excluindo pacotes aninhados. `[affected.rs:Tier]` `[affected.rs:answer]` `[affected.rs:dependents]` `[../trace.rs:runs]`

## Efeitos colaterais

A comparação de dependências pode ler arquivos do checkout e versões anteriores pelo Git. A resposta é construída em memória. `[affected.rs:attribute]` `[affected.rs:answer]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it -- affected:: kafka_lock::`.
