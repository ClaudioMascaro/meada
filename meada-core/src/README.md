# meada-core · Rust

Tipos e operações sobre manifestos, identidades, permissões e exportações.

## Interface

Valida entradas TOML, calcula diferenças de versões, autoriza mudanças e interpreta exportações do índice. A feature `wasm-bindgen` expõe essas operações ao navegador. `[manifest.rs:validate]` `[workspace.rs:diff]` `[authorize.rs:authorize_entries]` `[export.rs:Export]` `[lib.rs:wasm]`

O manifesto também descreve a base HTTP por variável de ambiente (`[[base_url]]`), o caminho do ambiente de um contêiner nos valores de Helm (`[[helm_env]]`) e o cliente de canal cuja chamada contém o tópico (`[[channel_client]]`). `[manifest.rs:BaseUrl]` `[manifest.rs:HelmEnv]` `[manifest.rs:ChannelClient]`

## Invariantes

O navegador usa a validação de importação, que aceita outras seções do manifesto ao validar as entradas. O campo `owner` admite até 256 caracteres. O cálculo de permissões recebe o papel, a concessão e as entradas como dados. `[wasm.rs:validate]` `[manifest.rs:validate_import]` `[manifest.rs:OWNER_MAX]` `[authorize.rs:authorize_entries]`

## Efeitos colaterais

A inicialização do módulo WASM seleciona o idioma das mensagens. O chamador fornece o texto dos manifestos e das exportações. `[wasm.rs:init]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada-core --features wasm-bindgen`.
