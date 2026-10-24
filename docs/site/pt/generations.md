# Gerações e retenção

**Ponteiro de geração e pins** (feature `generation-pointer`, que inclui `snapshot`;
`cargo install meada --features generation-pointer`). Com a seção `[generations]`, que exige
`[snapshot]` e `[valkey]`, o ponteiro da geração atual sai do store e vai para o Valkey, e a
frota inteira guarda o mesmo conjunto de gerações.


<!-- meada-graph:generation-lifecycle -->

```toml
[valkey]
url = "rediss://meada@cache.exemplo.internal:6379"   # TLS fora de loopback
password_file = "/run/secrets/valkey"                 # ou password_env
key_prefix = "meada:prod"                             # padrão "meada"

[generations]
keep = 3                                              # gerações retidas na frota, no mínimo 2
```

## Ponteiro e retenção

<details markdown="1" class="reference-details">
<summary>Retenção no store, poda local e rollback</summary>

O `meada snapshot publish` termina gravando, num só script, `{<key_prefix>}:current` (um hash
`{id, fence}`) e `{<key_prefix>}:retained` (uma lista de `<id>:<fence>`, da mais nova para a
mais antiga, cada id uma vez, cortada em `keep`), e não grava mais o `current` do store; o
fence é `0` sem a trava de refresh. Cada `meada snapshot pull` lê os dois do Valkey e confere
cada id (16 caracteres hexadecimais minúsculos) e cada fence (decimal) antes de mexer no nó;
então baixa toda geração retida que o nó não tem e apaga as que saíram da lista, nunca a atual
nem a que acabou de substituir. O arquivo de ponteiro do nó leva o fence lido com o id. Uma
entrada malformada, um ponteiro ausente ou uma lista vazia param o pull com um erro que nomeia
a chave, e o nó fica como estava. A geração atual é baixada e trocada primeiro: uma geração
retida que falha depois dela vira um aviso em stderr com o id e a causa, não bloqueia nada, e o
próximo pull tenta de novo. Use o mesmo `keep` no job e nos nós. `meada status --generations`
lê a geração atual do Valkey e confere que o store guarda o seu documento; a linha
`store: unreachable (...)` diz se falhou o Valkey ou o store.

</details>

## Fixar uma geração

Um run de workflow fixa as commits em que começou com um pin no caminho:
`/<identidade>@<geração>`, a geração sendo o que vem depois do último `@`, serve a geração
guardada, em qualquer nó, enquanto ela estiver retida, e `/<identidade>` continua servindo a
atual. Uma geração que o nó não guarda responde 404 `text/plain` nomeando a geração pedida, a
atual e as guardadas. Cada resposta HTTP nomeia a sua geração no cabeçalho `Meada-Generation`
(um 404 nomeia a atual) e cada resultado de ferramenta no `_meta`, `{"meada/generation":
"<id>"}`: o run lê a geração da primeira resposta e fixa as seguintes nela. As ferramentas, os
parâmetros e o texto das respostas não mudam. O proxy reverso repassa `Meada-Generation` nas
respostas. Uma chamada fixada ainda aberta numa geração que a retenção de um pull apaga pode
falhar no meio.
