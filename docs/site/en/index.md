---
hide:
  - toc
---

# The map between your repositories.

<div class="lead" markdown>
Meada reads code, finds who calls whom and connects repositories through the contracts they share. People and agents query the same map, with no LLM in indexing.
</div>

## Start with your code

Install, run at the root of a repository and connect your agent.

```sh
cargo install meada
meada index
meada mcp
```

<div class="reading-paths" markdown>
- [Install Meada](installation.md)

    Cargo, binaries, npm and containers.

- [Connect repositories](configuration.md)

    Declare peers, hosts and resources in meada.toml.

- [Query with your agent](mcp.md)

    Find symbols, read their context and follow a call.

- [Operate for a team](server.md)

    Mirrors, generations, authentication and the console.

</div>

<div class="tool-strip" markdown>
<div markdown>
[`search`](mcp.md)

Find a starting point.
</div>
<div markdown>
[`context`](mcp.md)

Understand a symbol’s connections.
</div>
<div markdown>
[`trace`](mcp.md)

Follow the chain across repositories.
</div>
</div>

## What is available

Server mode ([S1, #259](https://github.com/ClaudioMascaro/meada/issues/259)), the console ([S2, #293](https://github.com/ClaudioMascaro/meada/issues/293)) and identity automation ([S4, #319](https://github.com/ClaudioMascaro/meada/issues/319)) are delivered. Team ownership (S3) and named workspaces (S5) are planned. Each page distinguishes available behavior from the proposal.

[Read the capabilities and limits](capabilities.md) · [S3](workspaces.md) · [S4](enterprise.md) · [S5](named-workspaces.md)
