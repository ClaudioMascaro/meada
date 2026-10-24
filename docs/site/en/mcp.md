# Agents (MCP)

`meada mcp` speaks MCP over stdio. It answers about the first root (`roots`) the client sends,
in Claude Code the session's directory; without roots, about the directory it was started in,
which then must be inside the repository. If the client fails or does not answer `roots/list`
within 5 s, the call uses the last root it sent on that connection, or returns an error saying
so; it never falls back to the server's directory for that (which, for a session in a worktree,
would be another checkout). An answer with no local root counts as no roots (the server's
directory). In your agent's configuration (for example Claude Code's `.mcp.json`), using `npx`:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

With `meada` already on your `PATH`, use `"command": "meada", "args": ["mcp"]`.

There are three tools, always in English, and none of them writes:

- `search` finds a symbol, a route or a contract by name; with the kind `resource`, a resource
  by its kind or id; with the kind `channel`, a channel (queue or topic) by broker or name;
  with the kind `ambiguous` or `unresolved`, it lists the open items (see
  Decisions);
- `context` describes one symbol: its code read from disk, its callers, its callees and the
  contracts it touches; on a resource (`resource::postgres::orders`), it lists who writes and
  who reads it, with the symbol and line, or `(repository-level)` for the whole repository; on
  a channel (`channel::kafka::orders.created`), it lists producers and consumers, those that
  reach it through a wildcard or a binding included, with the level and the reason, and a side
  whose name Meada cannot evaluate shows as `(unresolved)` and is never linked;
- `trace` follows a chain of calls across repositories, downstream (`down`) or upstream (`up`);
  with `changes: uncommitted`, it starts from the working-tree diff, which covers the impact
  analysis of what was just edited.

Answers are compact lines under a token budget (2,000 by default, adjustable per call). Each
link between repositories carries its level and reason (`→[level: reason]`), and `trace` ends
with a footer: the weakest level of the chain, the number of hops, the repositories involved
and the tokens used.
