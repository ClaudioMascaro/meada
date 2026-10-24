# Refresh e publicação

A política escolhida é publicar o conjunto inteiro ou não publicar nada: falha em qualquer fetch ou indexação deve abortar a publicação e preservar a geração atual. A implementação e verificação dessa garantia estão [planejadas em #316](https://github.com/ClaudioMascaro/meada/issues/316). Hoje, o operador pode encadear o job com `sh -ec` para parar em uma falha; lock e fence já protegem a atualização do ponteiro. No [S4 planejado](enterprise.md), exclusões por elegibilidade serão decisões bem-sucedidas antes do fetch.

<!-- meada-graph:refresh -->

**Trava de refresh** (feature `refresh-lock`, que inclui `generation-pointer`, só Unix;
`cargo install meada --features refresh-lock`). Com a seção `[refresh_lock]`, que exige
`[generations]`, o refresh inteiro roda sob uma trava no Valkey, e cada aquisição ganha um
fence, de modo que um job sobreposto ou travado nunca troca uma geração nova por uma mais
antiga.

```toml
[refresh_lock]
ttl_seconds = 300                                     # padrão; no mínimo 10
```

```sh
meada snapshot lock --workspace ops/meada.workspace.toml -- \
  sh -c 'git -C api fetch && git -C api reset --hard origin/main &&
         meada index --workspace ops/meada.workspace.toml &&
         meada snapshot publish --workspace ops/meada.workspace.toml'
```

<details markdown="1" class="reference-details">
<summary>Aquisição, renovação e perda da trava</summary>

`meada snapshot lock` toma a trava com `SET {<key_prefix>}:lock <token aleatório> NX PX <ttl>`,
soma um em `{<key_prefix>}:fence` e roda o comando como filho, com `MEADA_REFRESH_LOCK` (o
token) e `MEADA_REFRESH_FENCE` no ambiente. A trava é renovada a cada terço do TTL (comparar e
estender) e liberada no fim (comparar e apagar); o código de saída é o do filho (128 mais o
sinal quando um sinal o encerrou). Uma trava já tomada sai com 1 nomeando a chave, sem rodar
nada. Quando uma renovação acha outro token, ou nenhuma deu certo por um TTL inteiro, a trava
está perdida: o grupo de processos do filho recebe SIGTERM (e SIGKILL 10 s depois, se ainda
estiver vivo), e `lock` sai com 1 dizendo que a perdeu. Um SIGTERM, SIGINT, SIGHUP ou SIGQUIT em `lock`
chega ao grupo do mesmo jeito; quando o filho termina, o que sobrou no grupo de processos recebe
SIGKILL (um neto que chama `setsid` sai do grupo e escapa), e no Linux o filho recebe SIGTERM se
`lock` morrer antes. A indexação continua sem rede: o `meada index` do
filho não abre conexão alguma.

</details>

## Publicação e recuperação do fence

<!-- meada-graph:fence-recovery -->

Com `[refresh_lock]`, `meada snapshot publish` exige as duas variáveis, e antes de cada envio
confere que a trava ainda guarda o token: se não, para sem enviar mais nada e sem gravar o
ponteiro. O script do ponteiro só grava quando a trava guarda o token e o fence é maior que o de
`{<key_prefix>}:current`, numa só operação. Os nós nunca trocam para uma geração de fence menor
que o do seu arquivo de ponteiro; sem o arquivo, tomam a atual seja qual for o fence. O Meada
nunca baixa nem apaga `{<key_prefix>}:fence`. Se um flush ou um failover do Valkey o perder, os
fences seguintes ficam menores que os registrados e a frota para: o publish recusado sai com 1
nomeando `{<key_prefix>}:fence`, o pull de um nó com fence maior sai com 1 sem trocar, e
`meada status --generations` nesse nó imprime uma linha `fence: travado: ...` (`fence: stuck: ...` fora do português) com os dois fences
(o código de saída não muda). Execute `meada snapshot fence recover --workspace <manifesto>`:
o comando recupera o contador sob a trava até o maior fence do pointer, das gerações retidas,
do workspace e dos heartbeats válidos, usando o relógio do Valkey para escolher os nós vivos.
O próximo publish usa um fence maior e destrava os nós. O `INCR` do Valkey tem teto de
9223372036854775807; a recuperação recusa pisos ou fences registrados acima de
9223372036854775806, e não consome a última aquisição disponível. Leitura e transporte
continuam aceitando fences `u64` exatos.
Se um nó isolado guarda um fence que não aparece num heartbeat válido, informe esse valor em
`--at-least <fence>`, ou apague seu `<local_dir>/current` para ele tomar a geração atual. Desligar `[refresh_lock]` depois
que os nós gravaram fences acima de 0 também trava a frota, já que sem a trava todo fence é 0:
apague o `<local_dir>/current` de cada nó.
