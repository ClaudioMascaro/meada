# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · [Español](README.es.md)

Meada lê os seus repositórios, monta o mapa de quem chama quem e costura um repositório no
outro pelos contratos que eles compartilham: rotas HTTP, filas, pacotes, recursos compartilhados.
Agentes de código, e gente também, consultam esse mapa por MCP. Nenhuma etapa da indexação usa
LLM.

## Instalação

```sh
cargo install meada
```

Para compilar esta árvore, use `cargo install --path . --locked` na raiz do clone.
Para uma versão publicada, há também:

- **Binário pronto**: baixe o arquivo da sua plataforma nas
  [releases do GitHub](https://github.com/ClaudioMascaro/meada/releases) e coloque `meada` no `PATH`.
- **Instalador shell** (Linux e macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.
- **npm/npx**: `npx meada --version`; o instalador baixa o binário da mesma versão.
- **Contêiner**: `docker run --rm ghcr.io/claudiomascaro/meada:latest --version`.

A publicação e a verificação do pacote npm ainda são etapas pendentes desta preparação.
Os exemplos com `npx` pressupõem uma versão publicada e verificada.

[Instalação](https://claudiomascaro.github.io/meada/installation/) · [GitHub releases](https://github.com/ClaudioMascaro/meada/releases)

## Conectar ao agente

No arquivo MCP do seu agente (por exemplo, `.mcp.json` no Claude Code):

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

Com o binário instalado no `PATH`, use `"command": "meada", "args": ["mcp"]`.
Abra a sessão no repositório que quer consultar. As três ferramentas, `search`, `context` e
`trace`, são de leitura. Veja [Agentes e MCP](https://claudiomascaro.github.io/meada/mcp/).

## Estado da versão

Esta árvore declara a versão **0.1.0** e prepara a **1.0**. A publicação da 1.0 e sua promessa
de estabilidade dependem dos gates de aceitação e da decisão de release do mantenedor;
esta documentação não afirma que esses gates passaram nem que a 1.0 já foi publicada.
Veja as [capacidades atuais](https://claudiomascaro.github.io/meada/capabilities/) e o
[roadmap](https://claudiomascaro.github.io/meada/roadmap/).

## Documentação

- [Configuração](https://claudiomascaro.github.io/meada/configuration/)
- [Agentes e MCP](https://claudiomascaro.github.io/meada/mcp/)
- [Comandos](https://claudiomascaro.github.io/meada/cli/)
- [Export JSON](https://claudiomascaro.github.io/meada/export/)
- [Modo servidor](https://claudiomascaro.github.io/meada/server/)
- [Console](https://claudiomascaro.github.io/meada/console/)
- [Capacidades e limites](https://claudiomascaro.github.io/meada/capabilities/)
- [Participar](https://claudiomascaro.github.io/meada/contributing/)

[Apache-2.0](LICENSE).
