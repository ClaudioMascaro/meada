# Meada com Docker Compose

[English](README.en.md) | [Español](README.es.md)

Exemplo local com dois nós, um Caddy com TLS e admissão por bearer token. Requer Docker
Compose v2 e imagens da mesma versão publicada. Somente `127.0.0.1:8443` fica publicado.
Os serviços não usam OIDC e a console fica desligada por padrão. Para acesso remoto, configure DNS,
certificado e regras de admissão próprios no Caddy.

```sh
cd deploy/compose
cp .env.example .env
# Edite .env: versão publicada, token aleatório e, no perfil generations, credenciais próprias.
docker compose config -q
docker compose up -d
docker compose logs -f local-indexer-one local-indexer-two
```

O perfil padrão, `local`, usa a imagem default e o layout A: cada indexer busca seu espelho
e indexa em volumes próprios; seu servidor só começa depois do sucesso desse job. Os dois
servidores leem manifesto, espelhos e índices por mounts somente de leitura. Eles podem
servir commits diferentes. Para atualizar, rode `docker compose run --rm local-indexer-one`
e depois `docker compose run --rm local-indexer-two`; os servidores detectam os índices novos.
Não execute dois indexers para o mesmo volume ao mesmo tempo.

`REPOSITORIES` contém URLs Git HTTP(S), separadas por espaço. Ao trocar repositórios, ajuste
também os blocos `[[repository]]` nos manifestos: identidade sem `.git` e caminho abaixo de
`/srv/mirrors`. O fetch faz parte do job; o `meada index` continua sem rede. Os dados vivem em
volumes Docker; `docker compose down` os preserva, e `down -v` os apaga.

## TLS, admissão e ferramentas

Espere os servidores ficarem `healthy` em `docker compose ps`. Copie a CA local do Caddy:

```sh
docker compose cp proxy:/data/caddy/pki/authorities/local/root.crt ./ca.crt
curl --cacert ca.crt -i https://localhost:8443/github.com/claudiomascaro/meada
# Esperado: HTTP 401, sem token.
printf 'Bearer token: '; read -r -s MEADA_TOKEN; printf '\n'; export MEADA_TOKEN
curl --cacert ca.crt -i https://localhost:8443/github.com/claudiomascaro/meada \
  -H "Authorization: Bearer $MEADA_TOKEN" -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' -H 'MCP-Protocol-Version: 2025-03-26' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"query":"main"}}}'
```

Esperado: HTTP 200 com resultado de `search`. Repetir a chamada alterna os nós por round robin.
Com uma identidade de símbolo do resultado, chame `trace` com `{"from":"<identidade>",
"direction":"down"}`; espere uma cadeia de chamadas e seu rodapé de certeza. Use uma função
que chame outra para obter saltos. O Caddy preserva `Authorization`, os cabeçalhos MCP e
`Meada-Generation`; não cria `Origin`. Não use `curl -k` nem registre o token em traces do shell.

## Gerações compartilhadas

Este perfil usa a imagem cloud e o layout B. Um job periódico busca, indexa e publica sob
`snapshot lock`. Valkey mantém pointer, retenção e fence; RustFS guarda bundles e documentos.
Ambos aceitam somente TLS na rede interna, sem portas publicadas. O job de certificados cria
uma CA privada, apaga sua chave ao terminar e monta em cada serviço só sua própria chave.
Os clientes conferem a CA via `ca_file`. As senhas vêm de `.env`, nunca do manifesto.

```sh
docker compose down
docker compose --profile generations config -q
docker compose --profile generations up -d
docker compose --profile generations logs -f generation-indexer first-pull-one first-pull-two
docker compose --profile generations ps
```

Selecione um perfil por vez; `--profile generations` substitui `COMPOSE_PROFILES=local` de
`.env`. O primeiro pull de cada nó precisa terminar antes de iniciar o servidor; depois um
companheiro faz `snapshot pull --every 30` no volume exclusivo desse nó. O servidor monta esse
volume somente para leitura. O healthcheck executa `status --generations`. `REFRESH_SECONDS`
controla o intervalo do job, 300 por padrão; a trava renova seu TTL durante o refresh e recebe
SIGTERM ao encerrar. Falhas do refresh aparecem no log e são tentadas de novo no próximo ciclo.

Para conferir a convergência, repita o `search` acima e anote `Meada-Generation` como `G1`:
as duas respostas alternadas devem nomear a mesma geração. Faça um commit novo no repositório
remoto configurado, espere o refresh e até 30 s de pull e repita: ambos devem passar a `G2`.
Chame o mesmo caminho com `@G1` no fim (`/.../meada@G1`): deve continuar respondendo com
`Meada-Generation: G1` enquanto G1 estiver entre as três gerações retidas. Um `trace` fixado
nessa URL deve manter o mesmo grafo mesmo depois da atualização. Geração fora da retenção
responde 404 com a geração atual e as guardadas. O exemplo não publica artefatos: as imagens
continuam vinculadas ao workflow de release.

## Console de gerenciamento, opcional

O overlay da console usa a imagem cloud e depende do perfil `generations`. Sem o overlay,
nenhum serviço, porta ou volume da console é iniciado. O exemplo admite um viewer local por
loopback, com operações desligadas. Antes de acesso remoto, configure OIDC ou admissão própria
no proxy; uma console exposta além dessa admissão revela o grafo.

Execute no diretório deste exemplo, com `.env` configurado. Edite `console.repositories.toml`
para o seed inicial, `REPOSITORIES` para o fetch e os bootstraps `job.workspace.toml` e
`console.workspace.toml` para o mesmo store, Valkey, prefixos e CA dos nós. O seed é uma ação
explícita do operador, feita uma vez; o deploy nunca reaplica uma versão automaticamente:

```sh
compose_console() {
  docker compose -f compose.yaml -f compose.console.yaml --profile generations --profile console "$@"
}
compose_console config -q
compose_console up -d bucket valkey
VERSION=$(compose_console run --rm --entrypoint meada generation-indexer workspace import \
  /etc/meada/repositories.toml --message "Initial managed workspace" --workspace /etc/meada/meada.workspace.toml)
compose_console run --rm --entrypoint meada generation-indexer workspace apply "$VERSION" \
  --workspace /etc/meada/meada.workspace.toml
compose_console up -d
compose_console logs -f generation-indexer console-first-pull console-pull console
```

O writer usa `[workspace] source = "store"`. O loop segura **uma** `snapshot lock` durante
`workspace pull`, fetch externo, `index` e `snapshot publish`. O fetch continua usando
`REPOSITORIES`, sem ler URLs ou caminhos arbitrários do editor. Ao adicionar um repositório ou
mudar seu caminho, ajuste essa lista, as credenciais e o mirror correspondente antes do refresh;
um mirror ausente impede publish, sem avançar o pointer. Aplicar uma versão apenas prepara o
próximo refresh; ela aparece na geração depois de publish e pull. Pare o writer para manter um
rollback de geração além do próximo refresh.

A console tem bootstrap, `index_dir`, `local_dir`, volume e `node_name = "console"` próprios.
Seu pull inicial termina antes da subida; um companion `snapshot pull --every 30` atualiza só
esse volume e escreve seu heartbeat. Os dois nós mantêm seus volumes e identidades separados.
`snapshot.ca_file` acrescenta raízes às do sistema; `valkey.ca_file` usa somente as do arquivo.

A rota TLS `https://localhost:8444` é própria, sem o bearer estático da porta MCP 8443.
O Caddy preserva `Host`, `Origin` e `Authorization`. Assets, `/api/v1/config` e `/auth/callback`
precisam chegar ao backend antes do login PKCE:

```sh
compose_console cp console-proxy:/data/caddy/pki/authorities/local/root.crt ./console-ca.crt
curl --cacert console-ca.crt https://localhost:8444/api/v1/config
curl --cacert console-ca.crt https://localhost:8444/auth/callback
```

Para OIDC, descomente e ajuste a política em `console.workspace.toml`: issuer, claims, grupos
e escopos iguais aos dos nós; `[oidc] public_url`/audience descrevem o MCP. `[ui] public_url`,
`client_id` e `audience` são próprios da console. Registre um cliente público sem segredo,
Authorization Code com PKCE S256 e redirect URI exato `https://localhost:8444/auth/callback`.
O browser precisa acessar discovery e token endpoint do issuer, com CORS para essa origem;
a troca de código ocorre diretamente entre eles. A API protegida exige o bearer do issuer.
Para operações, habilite `operations = true` e escolha `operator_groups` ou `operator_claim`,
apenas uma opção; grupos exigem `oidc.groups_claim`. O claim de operador deve ser booleano true.

O smoke local usa viewer por loopback, sem simular um IdP. OIDC e suas recusas são cobertos pelos
testes HTTP/Rust; o perfil OIDC do chart passa por lint/template. Valide com
`python3 ../../scripts/compose-check` a partir deste diretório. Encerre com
`compose_console down`; `down -v` também remove os dados e exige novo seed.
