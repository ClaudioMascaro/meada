# Coding standards

Read at review time: the `code-review` skill and every per-ticket reviewer apply these rules to
a diff. What the gate or a hook already enforces lives there, not here; each rule below is a
judgement call.

## Hostile input

A reader of a file the repository holds (a provider document, a config, a source file) treats
its bytes as **hostile**: a crafted file ends as a `meada status` line or an open part, and
indexing carries on.

- Recursion over nested input stops at a depth limit; deeper parts are open, or the file is
  unreadable at that line.
- Expansion (aliases, `$ref`s, variable substitution) is charged to a budget per file, and
  substitution runs once over the original text.
- A path read from a file or a config stays inside the repository root: relative, without `..`,
  and through no symlink.
- Each limit is a named constant with a `ponytail:` marker, listed under "Known ceilings" in
  `docs/agents/architecture.md`, and a unit test feeds it an input past the limit.
