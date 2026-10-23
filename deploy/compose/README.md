# Meada com Docker Compose

[English](README.en.md) | [Español](README.es.md)

Exemplo local com dois nós, um Caddy com TLS e admissão por bearer token. Requer Docker
Compose v2 e imagens da mesma versão publicada. Somente `127.0.0.1:8443` fica publicado.
Os serviços não usam OIDC e a console fica desligada. Para acesso remoto, configure DNS,
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
