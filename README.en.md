# Meada

[Português (Brasil)](README.md) · **English** · [Español](README.es.md)

> This is a translation. The Brazilian Portuguese [README](README.md) is the source; if they
> disagree, it wins, and a fix to this page is welcome.

Meada reads your repositories, maps who calls whom, and stitches one repository to the next
through the contracts they share: HTTP routes, queues, packages, environment variables. Coding
agents, and people too, query that map over MCP. No step of indexing uses an LLM.

> **Status:** under construction. There is nothing to install yet. The decisions made so far
> are in [`docs/adr`](docs/adr).

## Why another tool

Anyone who works across several services knows the bug rarely lives in a single repository. The
frontend calls a route that changed in the backend; the worker consumes a queue another team
renamed. Among today's code graph tools, some look at one repository at a time, some link
repositories only when the route text matches exactly, and some forbid commercial use.

Meada wants to follow the thread end to end, across as many repositories as it takes, and say
how confident it is in each link.

## Principles

- **Your code stays with you.** It runs on your machine, with no account, no API key and no
  telemetry.
- **No LLM to index.** The map comes from the code's syntax tree. No per-token bill in dollars;
  it works offline and fits on an ordinary laptop.
- **Portuguese first.** Documentation is born in Brazilian Portuguese; English and Spanish are
  translations. Issues and PRs are welcome in any of the three languages. The code is written
  in English so that anyone can read it.
- **Gather without blending.** "Not everything that comes together blends," taught Antônio
  Bispo dos Santos, known as Nêgo Bispo. Each repository keeps its own index; Meada only
  stitches the ends together.
- **Little noise for the agent.** Meada does not write to your `CLAUDE.md`, install hooks or
  touch your editor configuration unless you run a command for it. Answers are short.
- **Credit to those who came first.** Meada learns from GitNexus, CodeGraph,
  codebase-memory-mcp and kivgraph. What each one does is in
  [`docs/prior-art.md`](docs/prior-art.md).

## How to take part

The project is just starting, and the scope of the first version will be decided in the open.
You can already help:

- open an issue describing your cross-repository use case, in Portuguese, Spanish or English;
- review the translations: if the English or the Spanish sounds off, a fix is welcome;
- read and question the ADRs.

## The name

A *meada* is a skein of thread. *Puxar o fio da meada*, "pulling the skein's thread", means
finding the end that undoes the knot, which is what the tool tries to do when it follows a call
from one repository to the next.

## License

[Apache-2.0](LICENSE).
