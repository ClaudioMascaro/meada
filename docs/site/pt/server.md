# Modo servidor

> **Atenção.** Em modo servidor o Meada devolve trechos de código-fonte a quem consegue
> conectar, e por padrão não autentica ninguém: o proxy reverso do operador admite os clientes
> (TLS, tokens, SSO, política de rede); o [módulo OIDC](admission.md) é a alternativa opt-in. Sem `host`, o servidor escuta só no loopback; escutar além
> dele é uma escolha explícita do operador, e um proxy mal configurado expõe o código.
> O servidor MCP não confere o cabeçalho `Host` (um proxy com nome próprio funciona) e responde 403 a
> qualquer requisição com `Origin` (proteção contra navegadores e DNS rebinding), em qualquer
> caminho e antes de qualquer outra resposta. O proxy não deve expor o Meada além dos clientes
> que admite.


<!-- meada-graph:server-request -->

## Manifesto e mirrors

O modo servidor roda o Meada uma vez para uma organização: `meada mcp --listen` serve as
ferramentas por HTTP a partir de espelhos dos repositórios que um manifesto de workspace lista,
com cada índice fora do checkout. O manifesto `meada.workspace.toml` vive num repositório de
operações, versionado, e só é lido do caminho que `--workspace` dá:

```toml
index_dir = "/var/lib/meada"

[[repository]]
repo = "github.com/acme/web"
path = "/srv/mirrors/web"

[[repository]]
repo = "github.com/acme/api"
path = "/srv/mirrors/api"

[repository.serve]
hosts = ["api.acme.com"]
prefix = "/api"

[[repository.resource]]
kind = "postgres"
id = "orders"
role = "write"

[[repository.peer]]
repo = "github.com/acme/web"
base_url_env = "WEB_URL"
```

`index_dir` é obrigatório; `path` é absoluto ou relativo à pasta do manifesto. Os blocos
`serve`, `resource` e `peer` (o par sem `path`, pois o manifesto localiza todos) são opcionais e
valem para um espelho sem `meada.toml`: o `meada.toml` do próprio repositório vence, inteiro, e
os blocos do manifesto são ignorados (o `meada status` os nomeia). Um espelho é um checkout com
árvore de trabalho; um clone bare não tem o que indexar e é recusado.

## Indexar e servir

- `meada index --workspace <manifesto>` atualiza o índice de cada repositório, em
  `<index_dir>/<identidade>/index.db`. É o refresh do operador, rodado por um job de fetch ou
  por um webhook: o Meada nunca busca nem clona, e o servidor só lê. Depois de instalar um
  binário novo, rode `meada index --workspace` antes de reiniciar o servidor.
- `meada mcp --listen <endereço> --workspace <manifesto>` serve. O endereço é `host:porta`,
  `:porta` ou `porta`; o host é um IP ou `localhost`, nunca resolvido, e o padrão é o loopback.
  A primeira linha do stderr é `listening on http://<host>:<porta>`. SIGINT ou SIGTERM param o
  servidor: ele deixa de aceitar conexões, termina as respostas em andamento e sai com código 0. A espera tem limite fixo de 25 segundos,
  abaixo dos 30 s de carência padrão do Kubernetes: passado o limite, as conexões restantes são
  descartadas e a saída continua sendo 0.
- Cada repositório é um endpoint, `http://<host>:<porta>/<identidade>` (por exemplo
  `http://meada.internal:7333/github.com/acme/web`); o cliente configura uma URL por repositório
  em que trabalha. Qualquer outro caminho responde 404 com a lista das identidades servidas.
- O transporte HTTP é sem estado, com respostas em JSON: qualquer um de vários servidores sobre
  os mesmos índices responde a qualquer chamada.
- O servidor lê o manifesto e o `meada.toml` e o `meada.decisions.toml` de cada espelho uma vez,
  ao iniciar: mudar esses arquivos pede reiniciar o servidor. Uma mudança de código num espelho
  pede só `meada index --workspace`; a chamada seguinte já vê o índice novo.
- `context` imprime `index behind the mirror: run meada index --workspace` quando o arquivo de
  um trecho mudou depois da indexação. `trace` com `changes: uncommitted` é recusado, pois a
  árvore de trabalho está na máquina do cliente.
- `meada status --workspace <manifesto>` mostra o estado de cada índice, e
  `meada export --workspace <manifesto> --repo <identidade>` imprime o índice de um repositório,
  por exemplo para alimentar uma base de conhecimento.
