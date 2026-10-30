# Implantar

Três formas de pôr o Meada para rodar, da máquina de quem desenvolve ao cluster. Cada exemplo é
conferido pelo CI do repositório, pelo script indicado na seção.

## Docker, sem servidor

Na raiz do repositório:

```sh
docker run --rm --network none --user "$(id -u):$(id -g)" -v "${PWD%/*}:${PWD%/*}" -w "$PWD" ghcr.io/claudiomascaro/meada:1.0.0 index
docker run --rm --network none --user "$(id -u):$(id -g)" -v "${PWD%/*}:${PWD%/*}" -w "$PWD" ghcr.io/claudiomascaro/meada:1.0.0 status
```

- `--user` grava o `.meada/` com o seu usuário, não com o da imagem (uid 10001).
- `-v` monta a pasta que contém o repositório no mesmo caminho: os pares vizinhos entram no
  workspace, e o caminho que um cliente MCP envia como raiz existe dentro do contêiner. Para um
  repositório sem pares, `-v "$PWD:$PWD"` basta.
- `--network none`: indexar e consultar não usam rede.

O servidor MCP para o seu agente é a mesma linha com `-i` e `mcp`, na aba Docker de
[Agentes e MCP](mcp.md). `scripts/container-smoke` executa essa linha com um cliente MCP que
envia a raiz, a cada mudança na imagem.

## Docker Compose

`deploy/compose` sobe dois nós atrás de um Caddy com TLS e admissão por bearer token, e só
publica `127.0.0.1:8443`. O perfil padrão, `local`, busca e indexa os repositórios de
`REPOSITORIES` em volumes próprios; os perfis de gerações e da console são opcionais.

```sh
git clone --depth 1 --branch v1.0.0 https://github.com/ClaudioMascaro/meada.git
cd meada/deploy/compose
cp .env.example .env    # versão, token aleatório e, se quiser, REPOSITORIES
docker compose up -d
docker compose ps       # espere os servidores ficarem healthy
```

O teste com `curl` pelo TLS, a atualização dos índices e os outros perfis estão no
[README do Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/README.md).
`scripts/compose-check` valida todos os perfis com `docker compose config`.

## Helm

O chart sai com cada release em `oci://ghcr.io/claudiomascaro/charts/meada`, na mesma versão
do Meada. `values-minimal.yaml` sobe um nó, sem módulos cloud, que busca e indexa um repositório
público no init container:

```sh
curl -fsSLO https://raw.githubusercontent.com/ClaudioMascaro/meada/v1.0.0/deploy/helm/meada/values-minimal.yaml
helm install meada oci://ghcr.io/claudiomascaro/charts/meada --version 1.0.0 -f values-minimal.yaml
kubectl rollout status deployment/meada-meada
kubectl port-forward service/meada-meada 7333:7333
```

Em outro terminal, uma busca:

```sh
curl -s http://127.0.0.1:7333/github.com/claudiomascaro/meada \
  -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2025-06-18' \
  --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search","arguments":{"query":"main"}}}'
```

Troque os dois blocos de `values-minimal.yaml` pelos seus repositórios. Sem OIDC o servidor
admite quem alcança o Service: configure a admissão ([Autenticação e grants](admission.md))
antes de ligar o Ingress. Gerações, console e identidade empresarial estão no
[README do chart](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/README.md).
`scripts/helm-check` passa o `helm lint` e confere o que o chart renderiza com cada arquivo de
valores, este incluído.
