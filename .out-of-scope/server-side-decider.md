# Server-side decider

This project does not run a decider inside the MCP server, not even one that only suggests: no
`decider` cloud module, no `[decider]` section, no model pick or probability attached to an
`[ambiguous: ...]` or `[inferred: ...]` mark in an answer.

## Why this is out of scope

ADR 0002, invariant 2, says only `meada decide` sends a code excerpt to a model. A server that
calls a gateway on every answer would be a second path, even when the gateway is the operator's
own. Invariant 6 says answers depend on no model; a suggestion in an answer makes the answer
depend on one, and on its availability, latency and rate limits.

ADR 0006 and ADR 0011 say the server never runs `decide`, and only `decide` writes decisions. A
suggest-only module would keep the second rule and break the first.

ADR 0008 fixes the surface at three tools. A suggestion costs either a fourth tool or a field in
every `trace` or `context` answer, paid in tool-description tokens by every client.

The recipe that already works gives the same outcome as a reviewable diff: `meada decide
--decider` runs as a step on a clone, against the operator's gateway, and the resulting
`meada.decisions.toml` lands through a pull request. The next `meada index --workspace` replays
it and the server serves it as `decided`.

```sh
MEADA_DECIDER_PROTOCOL=chat MEADA_DECIDER_URL=https://llm.acme.internal \
MEADA_DECIDER_MODEL=<alias> MEADA_DECIDER_KEY=<key> meada decide --decider
```

The docs describe it under "Decide in a pipeline" on the server page.

## Prior requests

- #283: "Server mode: an opt-in decider module that suggests picks through the operator's model gateway"
