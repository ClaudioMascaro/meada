# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · [Español](README.es.md)

Meada lê os seus repositórios, monta o mapa de quem chama quem e costura um repositório no
outro pelos contratos que eles compartilham: rotas HTTP, filas, pacotes, recursos compartilhados.
Agentes de código, e gente também, consultam esse mapa por MCP. Nenhuma etapa da indexação usa
LLM.

## Instalação

Escolha o canal que combina com o seu ambiente. Todos entregam o mesmo binário `meada`.

```sh
# Cargo (compila com o toolchain Rust)
cargo install meada

# Instalador de shell (Linux e macOS)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh

# npm: baixa o binário da sua plataforma e o executa, sem instalar nada antes
npx meada --version
```

- **Binário pronto**: baixe o arquivo do seu sistema (Linux, macOS ou Windows, em x86_64 ou
  arm64) nas [releases do GitHub](https://github.com/ClaudioMascaro/meada/releases) e ponha
  `meada` no `PATH`.
- **Contêiner**: `ghcr.io/claudiomascaro/meada:<versão>`, para Linux amd64 e arm64.

A versão do pacote npm é sempre a da release. Para indexar, o Meada só precisa do binário e do
git. Os detalhes de cada canal, da console e do contêiner estão em
[Instalação](https://claudiomascaro.github.io/meada/installation/) e no
[guia do usuário](docs/user-guide.md) (seção 1, Instalação).

## Primeiros passos

Na raiz de um repositório (aqui `web`, que consome a rota de `api`):

```sh
meada index     # monta o índice em .meada/, do repositório e dos pares
meada status    # o estado de cada índice e os pares não encontrados
```

Para o Meada ligar `web` a `api`, o `meada.toml` de `web` nomeia o par:

```toml
[[peer]]
repo = "example.com/acme/api"       # remoto git de origin, sem esquema nem .git
base_url_env = ["API_URL"]          # variável de onde o cliente lê a URL base
```

A referência completa do `meada.toml` está no
[guia do usuário](docs/user-guide.md) (seção 4, referência do `meada.toml`).

## Conectar um agente (MCP)

`npx meada mcp` é a linha de lançamento do servidor MCP: o cliente a executa e fala com o Meada
por stdio. Na configuração do seu agente (por exemplo o `.mcp.json` do Claude Code):

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

No Claude Code, o mesmo em uma linha: `claude mcp add meada -- npx meada mcp`. Com `meada` já no
`PATH`, use `"command": "meada", "args": ["mcp"]`.

O agente ganha três ferramentas, `search`, `context` e `trace`, que só leem. Veja
[Agentes e MCP](https://claudiomascaro.github.io/meada/mcp/) e o
[guia do usuário](docs/user-guide.md) (seção 2, Conectar um agente).

## Status

Meada 1.0 entrega todo o escopo da v1 do [ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md):

- **Contratos**: rotas HTTP, recursos declarados, documentos OpenAPI e AsyncAPI, filas e
  tópicos, pacotes.
- **Níveis**: um nível em cada ligação, cadeias que atravessam repositórios e decisões com
  decisor opcional (`meada decide`).
- **Análises**: drift de forma, comunidades e processos.
- **Linguagens no 1.0**: TypeScript e JavaScript, Python, Go e Rust. Java, Kotlin, C# e Swift
  são trabalho planejado da Release 2 e chegam depois como versões 1.x (#26 a #29, ADR 0003).

**Promessa de estabilidade.** A partir do 1.0 vale o semver: uma mudança incompatível em um
destes itens pede uma versão major.

- os nomes, os parâmetros e a gramática de saída de `search`, `context` e `trace`;
- os comandos e as flags da CLI;
- os esquemas do `meada.toml` e do `meada.decisions.toml`;
- o formato do `meada export`.

Acrescentar um parâmetro opcional, um tipo de busca, uma chave de configuração, um campo do
export ou uma linguagem é mudança menor. Não entram na promessa o texto das mensagens da CLI, o
texto das descrições das ferramentas, o arquivo de índice e o layout de `.meada/` (reconstruídos
quando o esquema muda) e quais ligações o Meada encontra num código (melhorar a resolução é
correção, não quebra). O detalhe do que é congelado e do que não é está em
[Estabilidade](https://claudiomascaro.github.io/meada/stability/).

## Alimentar uma base de conhecimento

`meada export` imprime o índice de um repositório como JSON ordenado, bom para `diff` e para
alimentar uma base de conhecimento. Por repositório, ele traz os símbolos (identidade estável e
span), as chamadas com seus níveis, provedores e consumidores, links, imports, comunidades,
processos e pontos de entrada. Não traz texto-fonte: quem precisa do trecho lê o span no arquivo.

Um registro por símbolo, com comunidade, quem chama e quem é chamado, contratos e processos:

```sh
meada export > web.export.json
jq -c '. as $e | $e.symbols[] | .id as $id | {
  id, hash, span,
  community: ([$e.communities[]? | select(.members | index($id)) | .name] | first),
  callers: [$e.calls[] | select(.target == $id) | .caller],
  callees: [$e.calls[] | select(.caller == $id and .target) | .target],
  contracts: ([($e.providers[]?, $e.consumers[]?) | select(.site | startswith($id + "::")) | .contract] | unique),
  processes: [$e.processes[]? | select(any(.steps[]; .node == $id)) | .entry_point]
}' web.export.json

# duplicates: symbols that share a hash
jq '[.symbols[] | {id, hash}] | group_by(.hash) | map(select(length > 1))' web.export.json
```

Um registro por comunidade (`communities`) ou por contrato (`providers`, `consumers`, `links`)
segue as mesmas listas.

- **Fan-in** entre repositórios: conte, por alvo, as `calls`, os `links` (pelo `provider`) e os
  `imports` de todos os exports juntos.
- **Exposição**: os pontos de entrada em `processes` (`route`, `export`, `channel`, `program`), os
  nomes exportados e os consumidores de pacote (`packages`, `dependencies`, `imports`).

```sh
jq -s '[.[] | (.calls[]? | select(.target) | {site: .id, target, kind: "calls"}),
              (.links[]? | {site: .consumer, target: .provider, kind: "links"}),
              (.imports[]? | {site, target, kind: "imports"})]
  | unique_by([.kind, .site, .target])
  | group_by(.target)
  | map(. as $g | {target: $g[0].target}
      + ([$g[].kind] | group_by(.) | map({(.[0]): length}) | add))' *.export.json
```

O export é por repositório: uma organização roda `meada export` em cada um e junta os resultados
pela identidade dos símbolos. Embeddings e qualquer etapa com modelo ficam a jusante, fora do
Meada (invariante 1, [ADR 0002](docs/adr/0002-product-invariants.md)).

## Documentação

- [Guia do usuário](docs/user-guide.md)
- [Configuração](https://claudiomascaro.github.io/meada/configuration/)
- [Agentes e MCP](https://claudiomascaro.github.io/meada/mcp/)
- [Modo servidor](https://claudiomascaro.github.io/meada/server/)
- [Console](https://claudiomascaro.github.io/meada/console/)
- [Capacidades e limites](https://claudiomascaro.github.io/meada/capabilities/)
- [Estabilidade](https://claudiomascaro.github.io/meada/stability/)
- [Participar](https://claudiomascaro.github.io/meada/contributing/)

[Apache-2.0](LICENSE).
