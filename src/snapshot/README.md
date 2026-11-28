# Distribuição de snapshots e gerações

Distribuição de gerações de índices, retenção de versões e coordenação entre publicadores e nós de leitura.

## Interface

O cliente do armazenamento transfere objetos. O pull instala gerações no nó e a leitura serve a geração local. Com as features correspondentes, há ponteiro e retenção em Valkey, heartbeat, bloqueio de atualização e operações de versão. `[store.rs:Store]` `[pull.rs:pull]` `[serve.rs:Current]` `[generations.rs:point]` `[heartbeat.rs:beat]` `[lock.rs:acquire]` `[versions.rs:apply]`

## Invariantes

Os objetos baixados são validados antes da troca do ponteiro local. Com o bloqueio de atualização, a publicação precisa do token vigente e de um contador maior que o registrado no ponteiro para mover o ponteiro compartilhado. `[pull.rs:pull]` `[generations.rs:point]`

A leitura do ponteiro no Valkey valida os identificadores e contadores e rejeita uma lista de retenção vazia ou com mais de `keep + 8` entradas. Se o `meada.toml` do espelho for inválido, a consulta da configuração efetiva ainda retorna `held = true` e `own = true`, com `documents = false`. `[generations.rs:read]` `[generations.rs:KEPT_MAX]` `[serve.rs:facts]` `[../config.rs:read_config_up_to]`

## Efeitos colaterais

Transfere objetos por S3, grava arquivos locais e substitui o arquivo do ponteiro por renomeação. As funcionalidades de coordenação alteram chaves no Valkey e o comando de bloqueio também executa o processo filho solicitado. `[store.rs:Store]` `[pull.rs:replace]` `[operation.rs:pointer_set]` `[heartbeat.rs:beat]` `[lock.rs:run]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --features refresh-lock --test it -- snapshot:: generations::`.

Para exercitar os casos com serviços, carregue as variáveis impressas por `scripts/test-services up` antes do teste.
