---
name: meada
description: Code graph over MCP (Meada's `search`, `context`, `trace`). Use when finding where a symbol lives, who calls it, what a change touches, or how repositories connect through routes, queues, packages and shared resources.
---

# Meada

Meada's MCP tools answer from an index of the repository the session is in and its peers. Each
query refreshes the index from the files on disk, so answers include your edits. An answer is a
map: it names files and lines, and the files stay the territory.

## Steps

1. **Find** with `search`. `query` is a name, case-insensitive; `kind` narrows it (`symbol`,
   `route`, `contract`, `channel`, `resource`, `package`, `process`, `duplicate`). Done when you
   hold the identity (`<repository>::<path>::<name>`) or the contract (`http::GET::/users/{}`) of
   what you are after.
2. **Describe** with `context` on that identity: source, callers, callees, contracts,
   community, processes. Done when you know its callers and callees, each with its level.
3. **Follow** with `trace` when the question spans more than one hop: `from` plus `direction`,
   `down` for what it reaches or `up` for what reaches it. After an edit and before you report
   it done, `trace` with `changes: "uncommitted"` and no `from` gives the impact of the
   working-tree diff. Done when the footer's weakest level is one you can act on (below).
4. **Read** the lines the answer names before you edit them or state a claim about them.

Every tool takes `budget` (tokens, 2000 by default); raise it when a list comes back cut short.

## Reading an answer

A link between repositories reads `→[level: reason]`. The level is a name, from firmest to
weakest:

| Level | Meaning | What to do |
| --- | --- | --- |
| `exact` | the code or configuration proves it | act on it |
| `declared` | a declaration in `meada.toml` | as firm as that declaration |
| `decided` | recorded in `meada.decisions.toml`; the reason names who decided | act on it, citing the decision |
| `inferred` | a match by name or convention | read both ends to confirm |
| `ambiguous` | several candidates fit; all are listed | read the call site to pick one |

- **Weakest hop.** `trace` ends with `chain: <level> · N hops · N repos · N tokens`. A chain is
  as firm as its weakest hop, so report that level with any conclusion drawn from the chain.
- **Unproven absence.** An empty callers list is unproven, never proof of dead code: calls
  inside macros (`assert!`, `format!`, `vec!`) are not indexed. Before you delete, rename or
  call something unused, confirm with a text search across the workspace.
- **`(unresolved)`.** A URL or channel name computed at run time; Meada shows it and never links
  it. The code is the only source for where it goes.
- **Open items.** `search` with `kind` `ambiguous` or `unresolved` lists them, each with the
  `meada decide` command that resolves it. Deciding writes the user's `meada.decisions.toml`:
  propose the command to the user.
- **No matches.** The name may differ, or the language may not be indexed. `meada status` in a
  shell shows each repository of the workspace and its index state.
