# Workspaces e propostas no console

Operações do console sobre workspaces nomeados e servidor de propostas para agentes.

## Interface

O console oferece leitura de versões e histórico, configuração efetiva, prévia e mudanças de ciclo de vida. Com a feature `proposals`, o servidor MCP expõe catálogo, descoberta, consulta e proposta. `[effective.rs:configuration]` `[effective.rs:preview]` `[lifecycle.rs:operate]` `[lifecycle.rs:inspect]` `[proposals.rs:listen]`

## Invariantes

A sessão determina a identidade usada na autorização. As leituras filtram as entradas pela concessão do usuário e a prévia de configuração não salva uma versão. `[lifecycle.rs:readable_entries]` `[effective.rs:preview]`

O servidor MCP de propostas devolve HTTP 413 quando o corpo do pedido ultrapassa 80 KiB. `[proposals.rs:BODY_MAX]`

## Efeitos colaterais

As operações de ciclo de vida delegam a escrita ao armazenamento de workspaces. O servidor de propostas abre um listener HTTP e avalia propostas sem persistência. `[lifecycle.rs:operate]` `[proposals.rs:listen]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features cloud --test it ui::oidc::workspaces::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
