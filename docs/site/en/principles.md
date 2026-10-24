# Principles

- **Your code stays with you.** It runs on your machine, with no account, no API key and no
  telemetry.
- **No LLM to index.** The map comes from the code's syntax tree. No per-token bill in dollars;
  it works offline and fits on an ordinary laptop.
- **Deterministic.** The same code with the same configuration always gives the same map. When
  a link is ambiguous, the decision is recorded in a file versioned in git, made by a person
  or, if you choose, by a language model in a separate command. Indexing only re-reads that
  decision.
- **Portuguese first.** Documentation is born in Brazilian Portuguese; English and Spanish are
  translations. Issues and PRs are welcome in any of the three languages. The code is written
  in English so that anyone can read it.
- **Gather without blending.** "Not everything that comes together blends," taught Antônio
  Bispo dos Santos, known as Nêgo Bispo. Each repository keeps its own index; Meada only
  stitches the ends together.
- **Little noise for the agent.** Meada does not write to your `CLAUDE.md`, install hooks or
  touch your editor configuration unless you run a command for it. Answers are short.
- **Credit to those who came first.** Meada learns from GitNexus, CodeGraph,
  codebase-memory-mcp and kivgraph.
