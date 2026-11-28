# Decisões assistidas por modelo

Decisões assistidas por modelo na opção explícita `meada decide --decider`. Os adaptadores implementam os protocolos `chat` e `decisions`.

## Interface

A execução seleciona itens elegíveis, constrói o trecho de contexto e entrega as decisões aceitas ao callback. O protocolo de chat envia uma pergunta por requisição e o de decisões agrupa as perguntas. `[mod.rs:run]` `[chat.rs:request]` `[decisions.rs:request]`

O modo automático informa e pula itens sem candidatos ou com mais de 50 candidatos. `[mod.rs:MAX_CANDIDATES]` `[mod.rs:run]`

## Invariantes

Cada resposta precisa indicar uma chave oferecida, uma probabilidade entre zero e um e um identificador de modelo válido. `[validate.rs:proposal]`

## Efeitos colaterais

Lê configuração do ambiente e trechos do repositório. A execução pode enviar esses trechos por HTTP e escrever o resultado na saída fornecida. A visualização da requisição retorna antes do envio. `[mod.rs:from_env]` `[mod.rs:run]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it decider::`.
