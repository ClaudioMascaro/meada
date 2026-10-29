# Vários nós

Use vários nós para atender consultas em paralelo. Cada nó mantém sua própria cópia dos índices. Um proxy distribui as requisições; o transporte não precisa de afinidade de sessão (*sticky session*). Clientes no protocolo 2026-07-28 nem criam sessão.

O [exemplo Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/README.md) inclui dois nós, TLS e admissão por token no proxy. O [chart Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/README.md) usa **Deployment**, com init, readiness e encerramento gradual. O perfil com StatefulSet descrito abaixo exige adaptar os manifests; o chart atual não o gera.

## StatefulSet com S3 compartilhado

Este é um **perfil de referência**, usando os módulos existentes. StatefulSet dá identidade estável às réplicas e permite um volume próprio por pod. Ele não é obrigatório para o Meada: o chart entregue usa Deployment e `emptyDir` por pod.

Aqui, nó Meada significa uma réplica do servidor, executada em um pod. Um nó Kubernetes é a máquina que hospeda pods. StatefulSet não garante que suas réplicas ocupem máquinas diferentes; configure anti-affinity ou topology spread para distribuí-las entre workers e zonas, conforme a capacidade do cluster.

<!-- meada-graph:statefulset-s3 -->

O bucket e o prefixo S3 são compartilhados. Os arquivos SQLite ativos não são. Cada réplica tem seu próprio `local_dir`, com bundles extraídos, índices e ponteiro `current`. Um PVC por réplica pode preservar esses arquivos entre recriações; um volume efêmero exige repetir o pull inicial.

| Componente | Responsabilidade |
| --- | --- |
| Job de refresh | Busca os mirrors, indexa localmente e publica, sem sobreposição com outro job |
| S3 ou store compatível | Guarda documentos e bundles imutáveis sob o mesmo bucket/prefixo |
| Valkey | Coordena ponteiro de geração, retenção, lock, fence e heartbeats |
| Init de cada pod | Faz o primeiro `snapshot pull` antes de iniciar o servidor |
| Companion de cada pod | Repete o pull no `local_dir` exclusivo daquele pod |
| Servidor MCP | Consulta os índices locais com `--generations` |
| Proxy / Service | Distribui requisições entre réplicas prontas |

Configure bucket, prefixo e domínio Valkey iguais no job e nas réplicas. Dê um `snapshot.node_name` único a cada nó e à console; os nomes estáveis dos pods podem cumprir esse papel. O job mantém seus próprios mirrors, `index_dir` e `local_dir`.

S3 é acessado pela API de objetos, não como volume montado. O backend de snapshot entregue usa endpoint S3 compatível, com TLS fora de loopback. Não há backend `file://`.

Consulte [snapshots](snapshots.md) para configurar o store, [gerações](generations.md) para retenção e [refresh](refresh.md) para lock e fence. Qualquer falha de fetch ou indexação aborta toda a publicação e preserva a geração atual ([#316](https://github.com/ClaudioMascaro/meada/issues/316)).

## Escolher como distribuir os índices

<!-- meada-graph:node-layout -->

**Cada nó indexa seus espelhos.** O nó busca o código e roda `meada index --workspace` em um `index_dir` local. Esse layout não exige módulos de nuvem. Os nós podem responder com commits diferentes até todos atualizarem seus espelhos.

**Um job publica; os nós recebem.** O job busca e indexa os espelhos. Depois publica uma geração, o conjunto de bundles que contém código e índices. Cada nó baixa essa geração para seu próprio diretório e troca o ponteiro local. [Snapshots](snapshots.md) descreve a publicação e o pull.

Uma reconstrução troca `index.db.new.<pid>` por `index.db` com `rename`, sob `index.db.lock`. Assim, o leitor vê o índice anterior ou o novo inteiro. Só substitua um índice quando nenhum `meada index` estiver escrevendo nele: um `index.db-journal` ativo poderia ser interpretado como journal do arquivo novo.

## Armazenamento local e leitura

Não compartilhe um `index_dir` ativo entre escritor e leitores por NFS ou EFS. [SQLite sobre sistemas de arquivos de rede tem restrições de confiabilidade](https://sqlite.org/useovernet.html). Cada nó precisa de armazenamento local ou de um volume que só ele escreve.

O servidor pode ler `index_dir` e mirrors somente leitura, por exemplo preenchidos por init ou incluídos na imagem. Ele não cria arquivos ali. Mirrors de outro usuário são aceitos com `-c safe.directory`, apenas para os caminhos do manifesto. Isso exige git 2.38 ou posterior; em Ubuntu 22.04 e RHEL 8 com git antigo, configure `safe.directory` no sistema.

## Configurar proxy e prontidão

O proxy preserva `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` e todos os headers `Mcp-Param-*`. Pode rotear por esses valores, mas não alterá-los: o rmcp recusa divergências entre headers e corpo (SEP-2243). Preserve também o header de resposta `Meada-Generation`.

Não adicione `Origin`: o MCP responde 403 a requisições com esse header, por isso uma página de navegador precisa de um backend para chamá-lo. O servidor MCP não confere `Host`. `--listen` aceita IP ou `localhost`, sem resolver nomes.

Faça o init antes de liberar tráfego. No layout local, use uma sonda TCP ou `meada status --workspace <manifesto>`: saída 0 indica que todos os repositórios podem ser servidos, e 1 indica falha. O status espera até 50 ms por um commit e conta um índice travado como pronto. No perfil de snapshots, use [status com `--generations`](snapshots.md#prontidao).

`--listen` abre a porta somente após as verificações iniciais. O 404 em `/` lista identidades e não serve como health check.

## Executar um único escritor e encerrar réplicas

Evite jobs sobrepostos, por exemplo com `concurrencyPolicy: Forbid` no CronJob, e use o [lock de refresh](refresh.md). Configure `terminationGracePeriodSeconds` acima do limite de 25 s de encerramento por SIGTERM. O atraso de `preStop` deve dar ao balanceador tempo para retirar a réplica do tráfego.
