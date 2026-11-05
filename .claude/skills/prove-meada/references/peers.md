# A8: base URLs across peer workspaces (spec #683)

The A8 section of prove-meada. Spec #683 ("Base URLs through URL builders, fields and Backstage
config partials") is the answer key of every new A8 flow: `scripts/a8-cases.tsv` holds #684's
table of its expected lines, and a case a later ticket adds is checked against the issue
(`gh api repos/ClaudioMascaro/meada/issues/683 --jq .body`), never against a PR body or a golden
the same change wrote. The base binary is the answer key of the five controls and of every other
field of the exports.

## The workspaces

`scripts/a8-peers.sh gen DIR` writes neutral sibling repositories from heredocs, each with its
`meada.toml`, `git init` with an origin under `https://example.com/acme/` and one commit at fixed
dates. It needs only `meada`, `git` and `jq`, and no network. The shapes are the 1.2.0 dogfood
repro's (#684):

| Repository | Role | Holds |
|---|---|---|
| `api`, `other` | peers | Go `net/http`, both serve `GET /orders`: only `base_url_env` can pick `api` |
| `inventory` | peer | Go gin, serves `GET /clusters`, behind the Backstage proxy |
| `g8` | consumer | TS URL builders: a template string, `new URL(..)` with `searchParams` and `toString()`, `new URL(env)`, `.href`, `String(u)`, `${u}`, a path write; the `new URL(ref, base)` join (literal base, environment base, a parameter) |
| `g9` | consumer | Go cleanenv tag read inline and in a struct field (direct and through a `send()` wrapper); TS `config.getString` inline and in a readonly field set in the constructor |
| `g10a` to `g10c` | consumers | Backstage `config.getString`: the value inline, through `$include` with a `#` key path, through `$include` of a whole block |
| `g10d` to `g10g` | consumers | Backstage `discovery.getBaseUrl('proxy')`: `proxy.endpoints` inline, the legacy shape inline, each through `$include` |

Every consumer declares the three peers, `base_url_env = ["API_URL"]` on `api`.

## `a8-peers.sh check`

```bash
$S/a8-peers.sh check "$RUN/bin/meada-base" "$RUN/bin/meada-head" "$RUN/peers"
```

It generates one copy per binary (`$RUN/peers/base`, `$RUN/peers/head`), indexes and exports
every consumer with its binary (BLOCKED, exit 2, when an index or export fails or an export has
no symbol), and writes `$RUN/peers/peers.md`:

1. **Cases** (`scripts/a8-cases.tsv`): per case, the field (`contract` or `via`) of the consumer
   whose site names the function, at base and head, against the answer key, and the head's links
   of that consumer against the one link #683 names (provider repository, level, reason text).
   - `control`: the answer key, the base's field and the base's links, all equal.
   - `landed`: the answer key and the named link.
   - `pending`: the flow's A8 ticket has not landed; the case must read as the 1.2.0 column and as
     the base, links included, and does not count against the verdict. A pending case that meets
     the spec prints `MEETS SPEC: mark it landed` and counts as a difference until its row says
     `landed`.
2. **Everything else**: each export, without `commit`, `dirty` and the consumers, links and
   processes of the landed cases, equal to the base's: a pending case's other fields and links
   answer to the base.

Exit 0: no difference, 1: differences, 2: a run failed (BLOCKED). About 10 s with debug builds.

## Flows

Each A8 ticket that lands a flow turns its rows of `a8-cases.tsv` from `pending` to `landed`,
adds the cases its spec lines name beyond the repro's (`String(u)`, `` `${u}` ``, the
`new URL(ref, base)` joins, `$env`, the string endpoint, both proxy shapes, `meada status` on an
include not followed, a refresh after editing only a partial) to the generator and the table
through `/create-prove-it`, and adds its paths to SKILL.md's Scope row.

| Flow (#683) | Cases | Ticket | State |
|---|---|---|---|
| Controls: template string, cleanenv tag inline, `config.getString` inline, `proxy.endpoints` inline | 5 | existing | control |
| TS URL builders, one argument | 7 (`g8`) | #685 | landed |
| TS `new URL(ref, base)` join | 19 rows, 15 functions (`g8`) | #686 | landed (4 controls) |
| Go struct field | 2 (`g9`) | to come | pending |
| TS readonly field | 1 (`g9`) | #688 | landed |
| Backstage `$include` and `$env` | 3 (`g10b`, `g10c`, `g10h`) | #689 | landed |
| Backstage proxy shapes | 3 (`g10e` to `g10g`) | #690 | landed (`g10f`, `endpoints` inside an include, landed with #689) |

## Could fool you

- A case reads one field of the first consumer whose site names the function: a second call in
  the same function is not read. Each consumer function holds one call.
- The cases read the export (the CLI). The MCP answers on these workspaces (`context`, `trace`,
  open items in `search`, over stdio and HTTP) are not driven by the script: a change that reaches
  `src/mcp.rs` or the rendering runs `prove.py run --http` on the corpus beside it, and a checker
  that wants the peers' MCP answers runs `meada mcp` in `$RUN/peers/head/<consumer>` by hand.
- The peers (`api`, `other`, `inventory`) are not exported on their own: they reach the check
  through the consumers' providers and links.
- `pending` passes on 1.2.0's value, so a pending case proves only that nothing moved it. A ticket
  that lands a flow and leaves its rows `pending` gets `MEETS SPEC` (a difference), never a pass.
- The link check takes the head's links of the case's consumer only; a link the change adds to a
  control's consumer fails the control, one added elsewhere shows in "Everything else".
