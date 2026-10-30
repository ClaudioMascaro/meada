# Agents (MCP)

`meada mcp` speaks MCP over stdio. It answers about the first root (`roots`) the client sends,
in Claude Code the session's directory; without roots, about the directory it was started in,
which then must be inside the repository. If the client fails or does not answer `roots/list`
within 5 s, the call uses the last root it sent on that connection, or returns an error saying
so; it never falls back to the server's directory for that (which, for a session in a worktree,
would be another checkout). An answer with no local root counts as no roots (the server's
directory). In your agent's configuration (for example Claude Code's `.mcp.json`), in the form of your channel:

=== "npx"

    ```json
    {
      "mcpServers": {
        "meada": { "command": "npx", "args": ["-y", "@meada/meada", "mcp"] }
      }
    }
    ```

=== "meada on PATH"

    ```json
    {
      "mcpServers": {
        "meada": { "command": "meada", "args": ["mcp"] }
      }
    }
    ```

=== "Docker"

    ```json
    {
      "mcpServers": {
        "meada": {
          "command": "sh",
          "args": ["-c", "exec docker run -i --rm --network none --user \"$(id -u):$(id -g)\" -v \"${PWD%/*}:${PWD%/*}\" -w \"$PWD\" ghcr.io/claudiomascaro/meada:1.0.0 mcp"]
        }
      }
    }
    ```

    The client starts the server in the repository; the container mounts the folder that holds
    it, and its sibling peers, at the same path as the root the client sends, and runs as your user
    without a network. Linux and macOS only.

There are three tools, always in English, and none of them writes:

- `search` finds a symbol, a route or a contract by name; with the kind `resource`, a resource
  by its kind or id; with the kind `channel`, a channel (queue or topic) by broker or name;
  with the kind `duplicate`, groups of symbols whose text is identical in two or more places;
  with the kind `ambiguous` or `unresolved`, it lists the open items (see
  Decisions);
- `context` describes one symbol: its code read from disk, its callers, its callees and the
  contracts it touches; the annotations, attributes and decorators written on a definition (`@Override`,
  `[Fact]`, `@app.get("/x")`) are calls of the enclosing scope, so `context` lists them apart,
  in an `annotations:` section, each with the definition it decorates (the export's call site
  carries it as `decorates`); on a resource (`resource::postgres::orders`), it lists who writes and
  who reads it, with the symbol and line, or `(repository-level)` for the whole repository; on
  a channel (`channel::kafka::orders.created`), it lists producers and consumers, those that
  reach it through a wildcard or a binding included, with the level and the reason, and a side
  whose name Meada cannot evaluate shows as `(unresolved)` and is never linked; the candidates
  of an ambiguous call that are overloads of one method (one file, one class, one name) show
  as one set, `(one of N overloads)`, and `trace` writes them as `Class.name (N overloads)`;
  the call stays `ambiguous` and the export lists every candidate as before;
- `trace` follows a chain of calls across repositories, downstream (`down`) or upstream (`up`);
  with `changes: uncommitted`, it starts from the working-tree diff, which covers the impact
  analysis of what was just edited.

Answers are compact lines under a token budget (2,000 by default, adjustable per call). Each
link between repositories carries its level and reason (`→[level: reason]`), and `trace` ends
with a footer: the weakest level of the chain, the number of hops, the repositories involved
and the tokens used.

## Agent skill

The skill [`skills/meada/SKILL.md`](https://github.com/ClaudioMascaro/meada/blob/main/skills/meada/SKILL.md)
teaches an agent to choose between `search`, `context` and `trace` and to read the levels in an
answer: `inferred` and `ambiguous` are name matches, and a missing caller is unproven. It holds
for the version it was published with; fetch the one for your version.

- **Claude Code**: copy it to `~/.claude/skills/meada/` (every session) or to a project's
  `.claude/skills/meada/`:

    ```sh
    mkdir -p ~/.claude/skills/meada
    curl -fsSL https://raw.githubusercontent.com/ClaudioMascaro/meada/v1.0.0/skills/meada/SKILL.md -o ~/.claude/skills/meada/SKILL.md
    ```

- **Another agent that reads skill folders** (`SKILL.md` with frontmatter): copy the
  `skills/meada/` folder into its skills folder.
- **An agent without skills**: paste the body of `SKILL.md`, without the frontmatter, into the
  project's `AGENTS.md`.
