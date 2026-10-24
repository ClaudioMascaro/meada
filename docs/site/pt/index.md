---
hide:
  - toc
---

# O mapa entre seus repositórios.

<div class="lead" markdown>
Meada lê o código, encontra quem chama quem e conecta repositórios pelos contratos que compartilham. Pessoas e agentes consultam o mesmo mapa, sem LLM na indexação.
</div>

## Comece pelo seu código

Instale, execute na raiz de um repositório e conecte seu agente.

```sh
cargo install meada
meada index
meada mcp
```

<div class="reading-paths" markdown>
- [Instalar o Meada](installation.md)

    Cargo, binários, npm e contêineres.

- [Conectar repositórios](configuration.md)

    Declare peers, hosts e recursos em meada.toml.

- [Consultar com seu agente](mcp.md)

    Encontre símbolos, leia o contexto e siga uma chamada.

- [Operar para uma equipe](server.md)

    Mirrors, gerações, autenticação e console.

</div>

<div class="tool-strip" markdown>
<div markdown>
[`search`](mcp.md)

Encontre o ponto de partida.
</div>
<div markdown>
[`context`](mcp.md)

Entenda as conexões de um símbolo.
</div>
<div markdown>
[`trace`](mcp.md)

Siga a cadeia entre repositórios.
</div>
</div>

## O que está disponível

Modo servidor ([S1, #259](https://github.com/ClaudioMascaro/meada/issues/259)) e console ([S2, #293](https://github.com/ClaudioMascaro/meada/issues/293)) estão entregues. Ownership por equipe (S3), automação de identidade (S4) e workspaces nomeados (S5) estão planejados. Cada página separa o comportamento disponível da proposta.

[Consulte as capacidades e os limites](capabilities.md) · [S3](workspaces.md) · [S4](enterprise.md) · [S5](named-workspaces.md)
